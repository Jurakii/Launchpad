//! The window organizer (lib/organizer.js in 1.2): snaps other apps' windows
//! into layout zones from the panel along the top of each display
//! (`panel`), from the taskbar's "Snap to" menu, by dragging a window to
//! the top of a display (`drag`), or with the grid placer (`grid`). With no
//! window to move, the panel's zones and grid button open the window picker
//! (`menus`), which also holds the scenes menu. Scenes are in `scenes`.
//!
//! Everything is in physical pixels; the core modules that work in DIP
//! (strip, autohide) get coordinates scaled by the display's factor
//! (`to_dip`). State lives in a thread-local, like the other
//! parts; the windows exist only while needed (panels while the organizer
//! is on, the rest while shown).
pub mod dialogs;
pub mod drag;
pub mod grid;
pub mod menus;
pub mod panel;
pub mod scenes;

use super::{post, with, App};
use crate::win::display::{self, Monitor};
use crate::win::hotkey::Hotkey;
use crate::win::notify;
use crate::win::windowmgr::{self, events, Fail};
use launchpad_core::organizer::layouts as core_layouts;
use launchpad_core::organizer::zones::{self, Layout, Zone};
use launchpad_core::organizer::{Point, Rect};
use launchpad_core::settings::{organizer_options, OrganizerOptions};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{WA_INACTIVE, WM_ACTIVATE};

pub const HOTKEY_GRID: i32 = 2;

const ERR_GONE: &str = "That window has closed.";
const ERR_DENIED: &str = "Windows won't let Launchpad move this window because the app is running as administrator.";
const ERR_GENERIC: &str = "Launchpad couldn't move that window.";
pub const NOTHING_TO_MOVE: &str = "Click the window you want to move first.";
pub const LAYOUT_GONE: &str = "That layout no longer exists.";

pub fn fail_message(f: Fail) -> String {
    match f {
        Fail::Gone => ERR_GONE,
        Fail::Denied => ERR_DENIED,
        Fail::Failed => ERR_GENERIC,
    }
    .to_string()
}

#[derive(Default)]
pub struct State {
    pub enabled: bool,
    pub options: Option<OrganizerOptions>,
    /// The foreground window, from the events thread.
    pub focused: Option<events::Foreground>,
    events: Option<events::Events>,
    grid_hotkey: Option<Hotkey>,
    /// The layouts the panels and strip show (visible ones) and the gap.
    pub layouts: Vec<Layout>,
    pub gap: i32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(f)
}

/// A display's physical rects as core `Rect`s.
pub fn rect(r: &RECT) -> Rect {
    Rect::new(r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64)
}

pub fn to_rect(r: Rect) -> RECT {
    RECT { left: r.x.round() as i32, top: r.y.round() as i32, right: (r.x + r.width).round() as i32, bottom: (r.y + r.height).round() as i32 }
}

/// Physical → the display's DIP space (same origin, sizes divided by its
/// scaling), for core code with DIP thresholds.
pub fn to_dip(m: &Monitor, r: Rect) -> Rect {
    let (ox, oy, s) = (m.bounds.left as f64, m.bounds.top as f64, m.scale);
    Rect::new(ox + (r.x - ox) / s, oy + (r.y - oy) / s, r.width / s, r.height / s)
}

pub fn point_to_dip(m: &Monitor, p: Point) -> Point {
    let (ox, oy, s) = (m.bounds.left as f64, m.bounds.top as f64, m.scale);
    Point { x: ox + (p.x - ox) / s, y: oy + (p.y - oy) / s }
}

pub fn cursor() -> Point {
    let mut p = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
    }
    Point { x: p.x as f64, y: p.y as f64 }
}

/// The monitor a physical point is on (or nearest to).
pub fn monitor_at(p: Point) -> Option<Monitor> {
    let ms = display::monitors();
    let inside = ms.iter().find(|m| rect(&m.bounds).contains(p)).cloned();
    inside.or_else(|| {
        ms.into_iter().min_by_key(|m| {
            let c = rect(&m.bounds);
            let (dx, dy) = (c.x + c.width / 2.0 - p.x, c.y + c.height / 2.0 - p.y);
            (dx * dx + dy * dy) as i64
        })
    })
}

/// The monitor whose bounds are `bounds` (from `windowmgr::monitor`).
pub fn monitor_for(bounds: &RECT) -> Option<Monitor> {
    let ms = display::monitors();
    ms.iter().find(|m| m.bounds == *bounds).cloned().or_else(|| {
        monitor_at(Point { x: ((bounds.left + bounds.right) / 2) as f64, y: ((bounds.top + bounds.bottom) / 2) as f64 })
    })
}

pub fn find_zone<'a>(layouts: &'a [Layout], layout_id: &str, zone_id: &str) -> Option<(&'a Layout, &'a Zone)> {
    let l = layouts.iter().find(|l| l.id == layout_id)?;
    Some((l, l.zones.iter().find(|z| z.id == zone_id)?))
}

/// Where a zone lands on a display, in physical pixels: the gap is DIP in
/// layouts.json, so it's scaled here.
pub fn zone_rect(layout: &Layout, zone: &Zone, m: &Monitor, gap: i32) -> RECT {
    let gap = (gap as f64 * m.scale).round() as i32;
    to_rect(zones::zone_rect(layout, zone, rect(&m.work), gap))
}

pub fn place_in_zone(hwnd: i64, layout: &Layout, zone: &Zone, m: &Monitor, gap: i32) -> Result<(), String> {
    windowmgr::place(hwnd, zone_rect(layout, zone, m, gap), false).map(|_| ()).map_err(fail_message)
}

/// From the taskbar's "Snap to" menu: into the zone on the monitor the
/// window is already on, then to the front.
pub fn snap_window(app: &App, hwnd: i64, layout_id: &str, zone_id: &str) -> Result<(), String> {
    let data = core_layouts::load(&app.data_dir);
    let (layout, zone) = find_zone(&data.layouts, layout_id, zone_id).ok_or(LAYOUT_GONE)?;
    let wm = windowmgr::monitor(hwnd).map_err(fail_message)?;
    let m = monitor_for(&wm.bounds).ok_or(ERR_GENERIC)?;
    place_in_zone(hwnd, layout, zone, &m, data.gap)?;
    let _ = windowmgr::focus(hwnd);
    Ok(())
}

/// The visible layouts, for the "Snap to" menu.
pub fn visible_layouts(app: &App) -> Vec<Layout> {
    let data = core_layouts::load(&app.data_dir);
    core_layouts::visible(&data).into_iter().cloned().collect()
}

/// Shown when the user is in another app, so Launchpad's own dialogs
/// wouldn't be seen.
pub fn notify(app: &App, message: &str) {
    if let Some(t) = app.tray_icon.borrow().as_ref() {
        let _ = notify::show(t, "Launchpad", message);
    }
}

/// Settings changed (or at start): turns the organizer on or off and
/// applies its options. Safe to call again with the same settings.
pub fn configure(app: &Rc<App>) {
    let options = organizer_options(&app.settings());
    let enabled = options.enabled;
    let was = st(|s| s.enabled);
    st(|s| {
        s.enabled = enabled;
        s.options = Some(options.clone());
    });
    if enabled {
        start_events();
        register_grid_hotkey(app, &options.grid_hotkey);
    } else {
        st(|s| {
            s.grid_hotkey = None; // unregisters
            s.events = None;
            s.focused = None;
        });
        drag::cancel();
        grid::close(false);
        menus::close(false);
        drag::destroy_overlays();
    }
    if !options.snap_on_drag {
        drag::cancel();
    }
    load_layouts(app);
    panel::set_mode(app, if enabled { options.panel.as_str() } else { "off" });
    if enabled != was {
        panel::update_host(app);
    }
}

pub fn enabled() -> bool {
    st(|s| s.enabled)
}

pub fn snap_on_drag() -> bool {
    st(|s| s.enabled && s.options.as_ref().is_some_and(|o| o.snap_on_drag))
}

fn load_layouts(app: &App) {
    let data = core_layouts::load(&app.data_dir);
    let visible: Vec<Layout> = core_layouts::visible(&data).into_iter().cloned().collect();
    st(|s| {
        s.layouts = visible;
        s.gap = data.gap;
    });
}

/// A layout was added, changed, hidden or removed, or the gap changed: the
/// panels and the taskbar's Snap to menus follow.
pub fn layouts_changed(app: &Rc<App>) {
    load_layouts(app);
    app.refresh();
}

fn start_events() {
    if st(|s| s.events.is_some()) {
        return;
    }
    let events = events::start(|ev| post(move |app| on_event(app, ev)));
    match events {
        Ok(e) => st(|s| s.events = Some(e)),
        Err(e) => eprintln!("Organizer: {e}"),
    }
}

fn on_event(app: &Rc<App>, ev: events::WmEvent) {
    if !enabled() {
        return;
    }
    match ev {
        events::WmEvent::Foreground(f) => {
            let switched = st(|s| s.focused.as_ref().is_none_or(|o| o.hwnd != f.hwnd));
            st(|s| s.focused = Some(f));
            panel::push_content(app);
            if switched {
                panel::focus_changed(app);
            }
        }
        events::WmEvent::DragStart { hwnd } => drag::start(app, hwnd),
        events::WmEvent::MoveEnd { hwnd, cancelled } => drag::finish(app, hwnd, cancelled),
        events::WmEvent::WindowsChanged => {}
    }
}

/// The window the panel shows and acts on: while the grid, the scenes
/// menu or the picker is open it has the focus itself, so the window the
/// user was in before.
pub fn target() -> Option<events::Foreground> {
    grid::target().or_else(menus::previous).or_else(|| st(|s| s.focused.clone()))
}

fn register_grid_hotkey(app: &App, accel: &str) -> bool {
    let current = st(|s| s.grid_hotkey.as_ref().and_then(|h| h.accel().map(str::to_string)));
    if current.as_deref() == Some(accel) {
        return true;
    }
    let mut hk = st(|s| s.grid_hotkey.take()).unwrap_or_else(|| Hotkey::new(&app.msg, HOTKEY_GRID));
    let ok = hk.set(accel);
    if !ok {
        eprintln!("Could not register the grid hotkey \"{accel}\".");
    }
    st(|s| s.grid_hotkey = Some(hk));
    ok
}

/// From Settings: false (with 1.2's message) if another app has the keys;
/// the old ones then stay registered. Nothing to do while the organizer is off.
pub fn set_grid_hotkey(app: &Rc<App>, accel: &str) -> Result<(), String> {
    if !enabled() {
        return Ok(());
    }
    if !register_grid_hotkey(app, accel) {
        return Err(format!("\"{accel}\" is already in use by another app."));
    }
    panel::push_content(app);
    Ok(())
}

/// The grid hotkey's accelerator while registered.
pub fn grid_hotkey() -> Option<String> {
    st(|s| s.grid_hotkey.as_ref().and_then(|h| h.accel().map(str::to_string)))
}

pub fn on_grid_hotkey(app: &Rc<App>) {
    if let Err(e) = grid::open(app, None, None) {
        notify(app, &e);
    }
}

/// Displays changed: overlays and grids are sized to their display, so they
/// go; panels follow.
pub fn displays_changed(app: &Rc<App>) {
    if !enabled() {
        return;
    }
    drag::cancel();
    drag::destroy_overlays();
    grid::close(false);
    menus::close(false);
    panel::sync(app);
}

/// Quitting: the panels give their space back.
pub fn shutdown() {
    panel::shutdown();
    drag::destroy_overlays();
    grid::close(false);
    menus::close(false);
    st(|s| {
        s.grid_hotkey = None;
        s.events = None;
    });
}

/// For the windows that take the focus (grid, menus): `f(hwnd)` runs later
/// once the window loses it, if `counts()` says that loss counts.
pub fn close_on_blur(h: HWND, counts: fn() -> bool, f: fn(HWND)) {
    super::shell::subclass(h, move |hwnd, msg, wp, _| {
        if msg == WM_ACTIVATE && (wp.0 & 0xFFFF) as u32 == WA_INACTIVE && counts() {
            later(move |_| f(hwnd));
        }
        None
    });
}

/// Runs `f` with the app later, on the UI thread (for window callbacks
/// that would otherwise drop their own window).
pub fn later(f: impl FnOnce(&Rc<App>) + 'static) {
    super::later(f);
}

pub fn with_app<R>(f: impl FnOnce(&Rc<App>) -> R) -> Option<R> {
    with(f)
}
