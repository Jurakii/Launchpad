//! The organizer panel on every display (lib/panel.js in 1.2). "always":
//! docked as an AppBar, so maximized windows stop below it. "autoHide":
//! reserves nothing and comes down when the cursor rests at the top edge.
//! Either way it hides on a display a fullscreen app has taken over. The
//! panel on the display Launchpad's main window is on also carries the
//! window's top bar ("hosted").
//!
//! Re-entrancy: Explorer sends AppBar notifications to every docked bar
//! while one of them docks (inside `appbar::set`), so the message handler
//! never touches the state directly; it schedules work with `later`.
use super::{find_zone, menus, monitor_at, point_to_dip, rect, st as org, to_dip, zone_rect, NOTHING_TO_MOVE};
use crate::ui::shell::{self, Kind};
use crate::ui::{later, App, PanelCrumb, PanelLayout, PanelWindow, PanelZone, Theme};
use crate::win::appbar;
use crate::win::display::{self, Monitor};
use crate::win::windowmgr::{self, events};
use launchpad_core::organizer::{autohide, fullscreen, Point};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LRESULT, RECT};
use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW};
use windows::Win32::UI::Shell::{ABN_FULLSCREENAPP, ABN_POSCHANGED};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE, WM_NCACTIVATE, WM_NCPAINT};

pub const HEIGHT_DIP: f64 = 32.0;
const REDOCK_DELAY_MS: u64 = 100;
const AUTOHIDE_POLL_MS: u64 = 50;
const FULLSCREEN_RECHECK_MS: u64 = 2000;
const FOCUS_RECHECK_MS: u64 = 600;
const DOCK_RETRY_MS: u64 = 1000;
const DOCK_RETRY_MAX_MS: u64 = 15000;
const MESSAGE_MS: u64 = 3000;

struct Panel {
    monitor: Monitor,
    win: PanelWindow,
    hwnd: HWND,
    docked: bool,
    shown: bool,
    /// A fullscreen window covers its display (our own check).
    fullscreen: bool,
    /// Explorer's last ABN_FULLSCREENAPP for it: only a hint.
    explorer_fs: bool,
    auto: autohide::State,
    checking: bool,
    retry: Option<Timer>,
    failures: u32,
    message: Option<Timer>,
    /// The hosted crumbs' (x, width) in logical px, by index.
    crumb_geom: Vec<(f32, f32)>,
}

/// Launchpad's main window, as the panels need it.
#[derive(Clone, PartialEq, Default)]
struct Host {
    hwnd: i64,
    monitor: u32,
    fullscreen: bool,
}

#[derive(Default)]
struct Panels {
    mode: String,
    list: Vec<Panel>,
    dragging: bool,
    host: Option<Host>,
    crumbs: Vec<(String, String)>,
    /// What appbars.json last said, so it's only written on a change.
    recorded: Option<String>,
    autohide: Option<Timer>,
    clock: Option<Timer>,
    recheck: Option<Timer>,
    focus_recheck: Option<Timer>,
    focused_icon: Option<(String, Image)>,
    icon_fetching: Option<String>,
    /// Hosted-crumb index an item dragged from the grid is over.
    drop_crumb: Option<(u32, i32)>,
}

thread_local! {
    static PANELS: RefCell<Panels> = RefCell::new(Panels { mode: "off".into(), ..Default::default() });
}

fn ps<R>(f: impl FnOnce(&mut Panels) -> R) -> R {
    PANELS.with_borrow_mut(f)
}

fn record_file(app: &App) -> PathBuf {
    app.data_dir.join("appbars.json")
}

/// At start, before any panel docks: gives back the space a killed run
/// left reserved (the docked handles it listed in appbars.json).
pub fn release_leftovers(app: &App) {
    appbar::install_crash_cleanup();
    let Ok(text) = std::fs::read_to_string(record_file(app)) else { return };
    let list: Vec<i64> = serde_json::from_str(&text).unwrap_or_default();
    for h in &list {
        appbar::remove_stale(*h);
    }
    if !list.is_empty() {
        let _ = launchpad_core::safe_file::write_file_atomic(&record_file(app), "[]");
    }
    ps(|p| p.recorded = Some(String::new()));
}

fn record_docked(app: &App) {
    let list = appbar::docked();
    let key = list.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    if ps(|p| p.recorded.as_deref() == Some(key.as_str())) {
        return;
    }
    ps(|p| p.recorded = Some(key));
    if let Err(e) = launchpad_core::safe_file::write_file_atomic(&record_file(app), serde_json::to_string(&list).unwrap_or_default()) {
        eprintln!("Could not record the docked panels: {e}");
    }
}

pub fn set_mode(app: &Rc<App>, mode: &str) {
    if ps(|p| p.mode == mode) {
        return;
    }
    ps(|p| p.mode = mode.to_string());
    if mode == "off" {
        let gone: Vec<Panel> = ps(|p| {
            p.autohide = None;
            p.clock = None;
            p.recheck = None;
            p.focus_recheck = None;
            std::mem::take(&mut p.list)
        });
        for panel in gone {
            remove(panel);
        }
        record_docked(app);
        update_host(app);
        return;
    }
    sync(app);
    let n = ps(|p| p.list.len());
    for i in 0..n {
        ps(|p| p.list[i].auto = autohide::initial(false));
        place(app, i, false);
    }
    ps(|p| p.autohide = None);
    if mode == "autoHide" {
        let t = Timer::default();
        t.start(TimerMode::Repeated, Duration::from_millis(AUTOHIDE_POLL_MS), || {
            if let Some(app) = super::with_app(Rc::clone) {
                poll_autohide(&app);
            }
        });
        ps(|p| p.autohide = Some(t));
    }
    start_clock();
    check_fullscreen(app);
    update_host(app);
}

pub fn mode() -> String {
    ps(|p| p.mode.clone())
}

/// One panel per display; re-placed when its display moved or rescaled.
pub fn sync(app: &Rc<App>) {
    if ps(|p| p.mode == "off") {
        return;
    }
    let monitors = display::monitors();
    let gone: Vec<Panel> = ps(|p| {
        let (keep, gone): (Vec<Panel>, Vec<Panel>) =
            std::mem::take(&mut p.list).into_iter().partition(|panel| monitors.iter().any(|m| m.id == panel.monitor.id));
        p.list = keep;
        gone
    });
    for panel in gone {
        remove(panel);
    }
    for m in &monitors {
        let existing = ps(|p| p.list.iter().position(|panel| panel.monitor.id == m.id));
        match existing {
            Some(i) => {
                let (moved, reshaped) = ps(|p| {
                    let old = &p.list[i].monitor;
                    let moved = old.bounds != m.bounds || old.scale != m.scale;
                    let (ow, oh) = (old.work.right - old.work.left, old.work.bottom - old.work.top);
                    let reshaped = ow != m.work.right - m.work.left || oh != m.work.bottom - m.work.top;
                    p.list[i].monitor = m.clone();
                    (moved, reshaped)
                });
                if moved {
                    place(app, i, false);
                }
                if moved || reshaped {
                    push_one(app, i);
                }
            }
            None => create(app, m.clone()),
        }
    }
    record_docked(app);
    update_host(app);
}

fn bar_height(m: &Monitor) -> i32 {
    (HEIGHT_DIP * m.scale).round() as i32
}

fn top_rect(m: &Monitor) -> RECT {
    RECT { left: m.bounds.left, top: m.bounds.top, right: m.bounds.right, bottom: m.bounds.top + bar_height(m) }
}

fn create(app: &Rc<App>, m: Monitor) {
    let win = match shell::make(PanelWindow::new) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Organizer panel: {e}");
            return;
        }
    };
    crate::ui::prefs::apply_theme(app, &win.global::<Theme>());
    let r = top_rect(&m);
    win.window().set_position(slint::PhysicalPosition::new(r.left, r.top));
    win.window().set_size(slint::PhysicalSize::new((r.right - r.left) as u32, (r.bottom - r.top) as u32));
    let id = m.id;
    wire(&win, id);
    if let Err(e) = win.show() {
        eprintln!("Organizer panel: {e}");
        return;
    }
    shell::when_ready(&win, move |hwnd| later(move |app| ready(app, id, hwnd)));
    ps(|p| {
        p.list.push(Panel {
            monitor: m,
            win,
            hwnd: HWND::default(), // until winit has made it
            docked: false,
            shown: false,
            fullscreen: false,
            explorer_fs: false,
            auto: autohide::initial(false),
            checking: false,
            retry: None,
            failures: 0,
            message: None,
            crumb_geom: Vec::new(),
        })
    });
    let i = ps(|p| p.list.len() - 1);
    push_one(app, i);
}

/// The panel's HWND exists: styles, AppBar messages, then docking.
fn ready(app: &Rc<App>, id: u32, hwnd: HWND) {
    if with_panel(id, |p| p.hwnd = hwnd).is_none() {
        return; // gone meanwhile
    }
    shell::style(hwnd, Kind::NoActivate);
    let taskbar_created = appbar::taskbar_created();
    shell::subclass(hwnd, move |_, msg, wp, lp| {
        // With its clip region the panel has no themed frame, so Windows' own
        // frame painting (on activation changes) drew a white title bar over it.
        // The whole window is client area: there's no frame to paint.
        if msg == WM_NCACTIVATE {
            return Some(LRESULT(1));
        }
        if msg == WM_NCPAINT {
            return Some(LRESULT(0));
        }
        if msg == appbar::CALLBACK_MSG {
            let code = wp.0 as u32;
            if code == ABN_POSCHANGED {
                later(move |app| redock_soon(app, id));
            } else if code == ABN_FULLSCREENAPP {
                let on = lp.0 != 0;
                later(move |app| {
                    with_panel(id, |p| p.explorer_fs = on);
                    check_fullscreen(app);
                });
            }
            return Some(LRESULT(0));
        }
        if msg == taskbar_created {
            // Explorer restarted and forgot every AppBar.
            later(move |app| {
                if let Some(i) = index_of(id) {
                    place(app, i, true);
                }
            });
        }
        None
    });
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE); // until it's docked or revealed
    }
    if let Some(i) = index_of(id) {
        place(app, i, false);
        record_docked(app);
        update_host(app);
    }
}

fn remove(panel: Panel) {
    if panel.docked {
        appbar::remove(panel.hwnd.0 as i64);
    }
    let _ = panel.win.hide();
    drop(panel);
}

fn index_of(id: u32) -> Option<usize> {
    ps(|p| p.list.iter().position(|panel| panel.monitor.id == id))
}

fn with_panel<R>(id: u32, f: impl FnOnce(&mut Panel) -> R) -> Option<R> {
    ps(|p| p.list.iter_mut().find(|panel| panel.monitor.id == id).map(f))
}

fn redock_soon(app: &Rc<App>, id: u32) {
    let t = Timer::default();
    t.start(TimerMode::SingleShot, Duration::from_millis(REDOCK_DELAY_MS), move || {
        if let (Some(app), Some(i)) = (super::with_app(Rc::clone), index_of(id)) {
            place(&app, i, false);
        }
    });
    with_panel(id, |p| p.retry = Some(t));
    let _ = app;
}

/// Docks panel `i` ("always"), or parks it at the top edge ("autoHide").
fn place(app: &Rc<App>, i: usize, renew: bool) {
    let Some((h, m, docked, id)) = ps(|p| p.list.get(i).map(|x| (x.hwnd, x.monitor.clone(), x.docked, x.monitor.id))) else { return };
    if h.is_invalid() {
        return; // placed once it's ready
    }
    let hv = h.0 as i64;
    if mode() != "always" {
        if docked {
            appbar::remove(hv);
        }
        with_panel(id, |p| p.docked = false);
        record_docked(app);
        let r = top_rect(&m);
        shell::place(h, r);
        shell::clip_height(h, r.right - r.left, r.bottom - r.top);
        update_visibility(i);
        update_host(app);
        return;
    }
    match appbar::set(hv, top_rect(&m), renew) {
        Ok(rc) => {
            with_panel(id, |p| {
                p.docked = true;
                p.failures = 0;
                p.retry = None;
            });
            record_docked(app);
            shell::place(h, rc);
            shell::clip_height(h, rc.right - rc.left, rc.bottom - rc.top);
            update_visibility(i);
            update_host(app);
        }
        Err(_) => {
            // Retried with a growing delay, so an "always" panel isn't left hidden.
            let failures = with_panel(id, |p| {
                p.failures += 1;
                p.failures
            })
            .unwrap_or(1);
            let delay = (DOCK_RETRY_MS << (failures - 1).min(4)).min(DOCK_RETRY_MAX_MS);
            let t = Timer::default();
            t.start(TimerMode::SingleShot, Duration::from_millis(delay), move || {
                if let (Some(app), Some(i)) = (super::with_app(Rc::clone), index_of(id)) {
                    place(&app, i, renew);
                }
            });
            with_panel(id, |p| p.retry = Some(t));
        }
    }
}

fn update_visibility(i: usize) {
    let mode = mode();
    let Some((h, show, shown)) = ps(|p| {
        p.list.get(i).map(|x| {
            let show = !x.fullscreen
                && match mode.as_str() {
                    "always" => x.docked, // never over windows that haven't moved out of the way
                    "autoHide" => x.auto.revealed,
                    _ => false,
                };
            (x.hwnd, show, x.shown)
        })
    }) else {
        return;
    };
    if show != shown {
        ps(|p| p.list[i].shown = show);
        unsafe {
            let _ = ShowWindow(h, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
    if show {
        // Something (Explorer's AppBar handling, a fullscreen app leaving)
        // can take topmost away; it's put back after every show.
        let _ = windowmgr::ensure_topmost(h.0 as i64);
        // Shown or moved: draw it all again, or what Windows left there
        // (a blank frame) stays until something in the panel changes.
        unsafe {
            let _ = RedrawWindow(Some(h), None, None, RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
        }
    }
}

/// The handles of Launchpad's own windows, which never count as a
/// fullscreen app (the main window in fullscreen has the panel as its top bar).
fn own_windows(app: &App) -> Vec<i64> {
    let mut own: Vec<i64> = app.hwnd().map(|h| h.0 as i64).into_iter().collect();
    own.extend(ps(|p| p.list.iter().map(|x| x.hwnd.0 as i64).collect::<Vec<_>>()));
    own
}

/// Hides the panels of displays a fullscreen window has taken over.
pub fn check_fullscreen(app: &Rc<App>) {
    if ps(|p| p.list.is_empty()) {
        return;
    }
    let list = windowmgr::fullscreen(&own_windows(app));
    let reply = windowmgr::fullscreen_reply(&list);
    let n = ps(|p| p.list.len());
    for i in 0..n {
        ps(|p| {
            let x = &mut p.list[i];
            x.fullscreen = fullscreen::decide(Some(&reply), rect(&x.monitor.bounds), x.fullscreen, x.explorer_fs);
        });
        update_visibility(i);
    }
    // A fullscreen window closing on a display the user isn't working on
    // changes nothing Launchpad hears about, so look again now and then.
    let again = ps(|p| p.list.iter().any(|x| x.fullscreen || x.explorer_fs));
    if again {
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(FULLSCREEN_RECHECK_MS), || {
            if let Some(app) = super::with_app(Rc::clone) {
                check_fullscreen(&app);
            }
        });
        ps(|p| p.recheck = Some(t));
    } else {
        ps(|p| p.recheck = None);
    }
}

/// Another window came to the front: a fullscreen app may have come or gone.
/// Apps usually go fullscreen just after, so it's checked again shortly.
pub fn focus_changed(app: &Rc<App>) {
    if ps(|p| p.list.is_empty()) {
        return;
    }
    check_fullscreen(app);
    let t = Timer::default();
    t.start(TimerMode::SingleShot, Duration::from_millis(FOCUS_RECHECK_MS), || {
        if let Some(app) = super::with_app(Rc::clone) {
            check_fullscreen(&app);
        }
    });
    ps(|p| p.focus_recheck = Some(t));
}

/// The drag strip takes the top of the display; an auto-hiding panel keeps
/// out of its way.
pub fn set_dragging(on: bool) {
    ps(|p| p.dragging = on);
    if !on || mode() != "autoHide" {
        return;
    }
    let n = ps(|p| p.list.len());
    for i in 0..n {
        ps(|p| p.list[i].auto = autohide::initial(false));
        update_visibility(i);
    }
}

fn poll_autohide(app: &Rc<App>) {
    let c = super::cursor();
    let now = Instant::now().elapsed().as_millis() as i64 + clock_base();
    let dragging = ps(|p| p.dragging);
    let n = ps(|p| p.list.len());
    for i in 0..n {
        let (m, state, fs, checking) = ps(|p| {
            let x = &p.list[i];
            (x.monitor.clone(), x.auto, x.fullscreen, x.checking)
        });
        let bounds = to_dip(&m, rect(&m.bounds));
        let panel = launchpad_core::organizer::Rect::new(bounds.x, bounds.y, bounds.width, HEIGHT_DIP);
        let next = autohide::step(
            state,
            autohide::Sample { cursor: point_to_dip(&m, c), bounds, panel, now, can_reveal: !dragging && !fs && !checking },
        );
        if next.revealed && !state.revealed {
            // No AppBar notification reaches an auto-hiding panel, so a
            // fullscreen app could have taken over unnoticed: look first.
            ps(|p| p.list[i].checking = true);
            check_fullscreen(app);
            ps(|p| {
                let x = &mut p.list[i];
                x.checking = false;
                x.auto = autohide::initial(!x.fullscreen && !dragging);
            });
        } else {
            ps(|p| p.list[i].auto = next);
        }
        update_visibility(i);
    }
}

fn clock_base() -> i64 {
    thread_local!(static BASE: (Instant, i64) = (Instant::now(), launchpad_core::time::now_ms()));
    BASE.with(|(i, ms)| ms - i.elapsed().as_millis() as i64)
}

// ---------- content ----------

fn layouts_model() -> ModelRc<PanelLayout> {
    let layouts = org(|s| s.layouts.clone());
    let rows: Vec<PanelLayout> = layouts
        .iter()
        .map(|l| PanelLayout {
            id: l.id.as_str().into(),
            name: l.name.as_str().into(),
            cols: l.cols,
            rows: l.rows,
            zones: ModelRc::new(VecModel::from(
                l.zones
                    .iter()
                    .map(|z| PanelZone { id: z.id.as_str().into(), name: z.name.as_str().into(), x: z.x, y: z.y, w: z.w, h: z.h })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// What the panels show, for all of them.
pub fn push_content(app: &App) {
    let n = ps(|p| p.list.len());
    for i in 0..n {
        push_one(app, i);
    }
}

fn push_one(app: &App, i: usize) {
    let Some((win, m, hosted)) = ps(|p| {
        let host = p.host.as_ref().map(|h| h.monitor);
        p.list.get(i).map(|x| (x.win.clone_strong(), x.monitor.clone(), host == Some(x.monitor.id)))
    }) else {
        return;
    };
    crate::ui::prefs::apply_theme(app, &win.global::<Theme>());
    let wa = rect(&m.work);
    win.set_aspect((wa.width / wa.height.max(1.0)) as f32);
    win.set_layouts(layouts_model());
    win.set_bar_height(HEIGHT_DIP as f32);
    let target = super::target();
    let shown = target.as_ref().filter(|t| t.visible && t.kind == events::Kind::App);
    win.set_has_focused(shown.is_some());
    win.set_focused_title(shown.map(|t| t.title.as_str()).unwrap_or("").into());
    win.set_focused_icon(shown.and_then(|t| icon_for(app, t.exe.as_deref())).unwrap_or_default());
    // Nothing to move: a click opens the picker instead.
    let pick = !target.as_ref().is_some_and(|t| t.snappable && t.kind == events::Kind::App);
    win.set_pick(pick);
    let keys = super::grid_hotkey().map(|k| format!(" ({})", k.replace("CommandOrControl", "Ctrl"))).unwrap_or_default();
    win.set_grid_tip(
        if pick {
            format!("Grid placer{keys}: pick a window, then size it to any block of cells")
        } else {
            format!("Grid placer{keys}: size the window to any block of cells")
        }
        .into(),
    );
    win.set_hosted(hosted);
    if hosted {
        let crumbs = ps(|p| p.crumbs.clone());
        win.set_crumbs(ModelRc::new(VecModel::from(
            crumbs.iter().map(|(id, name)| PanelCrumb { id: id.as_str().into(), name: name.as_str().into() }).collect::<Vec<_>>(),
        )));
        win.set_fullscreen(ps(|p| p.host.as_ref().is_some_and(|h| h.fullscreen)));
    }
    let drop = ps(|p| p.drop_crumb.filter(|(id, _)| *id == m.id).map(|(_, i)| i).unwrap_or(-1));
    win.set_drop_crumb(drop);
    set_clock(&win);
}

/// The focused window's exe icon, fetched once per exe off the UI thread.
pub(super) fn icon_for(app: &App, exe: Option<&str>) -> Option<Image> {
    let exe = exe?;
    if let Some(img) = ps(|p| p.focused_icon.as_ref().filter(|(e, _)| e == exe).map(|(_, i)| i.clone())) {
        return Some(img);
    }
    if ps(|p| p.icon_fetching.as_deref() == Some(exe)) {
        return None;
    }
    ps(|p| p.icon_fetching = Some(exe.to_string()));
    let exe = exe.to_string();
    let _ = app;
    std::thread::spawn(move || {
        let rgba = crate::win::image::Wic::new().ok().and_then(|w| w.file_icon(std::path::Path::new(&exe), 32).ok());
        crate::ui::post(move |app| {
            ps(|p| {
                if p.icon_fetching.as_deref() == Some(exe.as_str()) {
                    p.icon_fetching = None;
                }
            });
            if let Some(rgba) = rgba {
                ps(|p| p.focused_icon = Some((exe, crate::ui::images::to_image(rgba))));
                push_content(app);
            }
        });
    });
    None
}

fn set_clock(win: &PanelWindow) {
    use windows::core::w;
    use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS};
    let (mut d, mut t) = ([0u16; 64], [0u16; 64]);
    let dn = unsafe { GetDateFormatEx(None, Default::default(), None, w!("ddd, MMM d"), Some(&mut d), None) };
    let tn = unsafe { GetTimeFormatEx(None, TIME_NOSECONDS, None, None, Some(&mut t)) };
    win.set_clock_date(crate::win::from_wide(&d[..dn.max(0) as usize]).into());
    win.set_clock_time(crate::win::from_wide(&t[..tn.max(0) as usize]).into());
}

/// One timer for every panel's clock, firing just after each minute turns.
fn start_clock() {
    fn arm() {
        let now = launchpad_core::time::now_ms();
        let wait = 60_000 - now.rem_euclid(60_000) + 50;
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(wait as u64), || {
            let wins: Vec<PanelWindow> = ps(|p| p.list.iter().map(|x| x.win.clone_strong()).collect());
            for w in &wins {
                set_clock(w);
            }
            if ps(|p| p.mode != "off") {
                arm();
            }
        });
        ps(|p| p.clock = Some(t));
    }
    arm();
}

/// Text where a display's panel shows the focused window's title, for a few
/// seconds (long ones longer): the result of applying a scene, say.
pub fn show_message(monitor: u32, text: &str) {
    let ms = (text.chars().count() as u64 * 60).clamp(MESSAGE_MS, 8000);
    let Some(win) = with_panel(monitor, |p| p.win.clone_strong()) else { return };
    win.set_message(text.into());
    let t = Timer::default();
    t.start(TimerMode::SingleShot, Duration::from_millis(ms), move || {
        if let Some(w) = with_panel(monitor, |p| {
            p.message = None;
            p.win.clone_strong()
        }) {
            w.set_message("".into());
        }
    });
    with_panel(monitor, |p| p.message = Some(t));
}

// ---------- clicks ----------

fn wire(win: &PanelWindow, id: u32) {
    win.on_crumb_geom(move |i, x, w| {
        let i = i.max(0) as usize;
        // Runs during layout: only a note, no borrow held elsewhere can clash.
        PANELS.with(|cell| {
            if let Ok(mut p) = cell.try_borrow_mut() {
                if let Some(panel) = p.list.iter_mut().find(|panel| panel.monitor.id == id) {
                    if panel.crumb_geom.len() <= i {
                        panel.crumb_geom.resize(i + 1, (0.0, 0.0));
                    }
                    panel.crumb_geom[i] = (x, w);
                }
            }
        });
    });
    win.on_zone_clicked(move |layout, zone, x| {
        let (layout, zone) = (layout.to_string(), zone.to_string());
        later(move |app| {
            super::drag::preview(app, id, None);
            if let Err(e) = snap_from_panel(app, id, &layout, &zone, x) {
                show_message(id, &e);
            }
        });
    });
    win.on_zone_hover(move |layout, zone| {
        let (layout, zone) = (layout.to_string(), zone.to_string());
        later(move |app| {
            let can = super::target().is_some_and(|t| t.snappable && t.kind == events::Kind::App);
            if can {
                super::drag::preview(app, id, Some((layout, zone)));
            }
        });
    });
    win.on_zone_leave(move || later(move |app| super::drag::preview(app, id, None)));
    win.on_grid_clicked(move |x| {
        later(move |app| {
            if let Err(e) = grid_from_panel(app, id, x) {
                show_message(id, &e);
            }
        })
    });
    win.on_scenes_clicked(move |x, w| later(move |app| menus::open_scenes(app, id, x + w)));
    win.on_toolbar(move |action| {
        let action = action.to_string();
        later(move |app| toolbar(app, &action));
    });
    win.on_crumb_clicked(move |crumb| {
        let crumb = crumb.to_string();
        later(move |app| {
            app.show(false);
            crate::ui::grid::open_island(app, Some(crumb));
        });
    });
    win.on_crumb_menu(move |crumb, _, _| {
        // The island's menu opens in the main window, as in 1.2.
        let crumb = crumb.to_string();
        later(move |app| {
            app.show(true);
            crate::ui::grid::open_island(app, Some(crumb.clone()));
            crate::ui::grid::open_island_menu(app, &crumb);
        });
    });
}

/// The panel's logical x → a screen x, for anchoring the picker under it.
fn anchor_x(id: u32, x: f32) -> i32 {
    with_panel(id, |p| p.monitor.bounds.left + (x as f64 * p.monitor.scale).round() as i32).unwrap_or(0)
}

pub fn monitor_of(id: u32) -> Option<Monitor> {
    with_panel(id, |p| p.monitor.clone())
}

/// The bottom edge of a display's panel (or its display's top), for menus.
pub fn bottom_of(id: u32) -> i32 {
    with_panel(id, |p| p.monitor.bounds.top + bar_height(&p.monitor)).unwrap_or(0)
}

fn snap_from_panel(app: &Rc<App>, id: u32, layout_id: &str, zone_id: &str, x: f32) -> Result<(), String> {
    let m = monitor_of(id).ok_or("Not a panel.")?;
    let (layouts, gap) = org(|s| (s.layouts.clone(), s.gap));
    let (layout, zone) = find_zone(&layouts, layout_id, zone_id).ok_or(super::LAYOUT_GONE)?;
    // Always the picker, unless the grid is open: then its window was
    // already chosen and the zone moves that one.
    let Some(target) = super::grid::target() else {
        let heading = format!("Move a window to {}: {}", layout.name, if zone.name.is_empty() { &zone.id } else { &zone.name });
        return menus::open_picker(app, id, anchor_x(id, x), menus::PickFor::Zone { layout: layout_id.into(), zone: zone_id.into() }, heading);
    };
    windowmgr::place(target.hwnd, zone_rect(layout, zone, &m, gap), false).map_err(super::fail_message)?;
    super::grid::close(true);
    let _ = app;
    Ok(())
}

fn grid_from_panel(app: &Rc<App>, id: u32, x: f32) -> Result<(), String> {
    let m = monitor_of(id).ok_or("Not a panel.")?;
    if super::grid::is_open() {
        return super::grid::open(app, Some(m), None); // move it here, or close it
    }
    let snappable = super::target().filter(|t| t.snappable && t.kind == events::Kind::App);
    if snappable.is_none() {
        return menus::open_picker(app, id, anchor_x(id, x), menus::PickFor::Grid, "Pick a window for the grid".into());
    }
    super::grid::open(app, Some(m), None).map_err(|e| if e.is_empty() { NOTHING_TO_MOVE.into() } else { e })
}

/// The top bar's buttons on the hosting panel. Only the ones that open a
/// dialog bring the window to the front; navigation leaves it where it is.
fn toolbar(app: &Rc<App>, action: &str) {
    match action {
        "fullscreen" => {
            if let Some(w) = app.window.borrow().as_ref() {
                let on = !w.window().is_fullscreen();
                w.window().set_fullscreen(on);
            }
            later(update_host);
        }
        "media" => crate::ui::grid::open_media(app),
        "layouts" => {
            app.show(true);
            super::dialogs::open_layouts(app);
        }
        "settings" => {
            app.show(true);
            crate::ui::settings::open(app, None);
        }
        "newIsland" => {
            app.show(true);
            crate::ui::modals::new_island(app, crate::ui::grid::current_island(), None);
        }
        "addApp" => {
            app.show(true);
            crate::ui::modals::add_app(app, crate::ui::grid::current_island(), None);
        }
        _ => {}
    }
}

// ---------- Launchpad's top bar ----------

/// The main window moved, was shown, hidden or dropped, or went in or out
/// of fullscreen: works out which panel (if any) carries its top bar and
/// tells both sides.
pub fn update_host(app: &Rc<App>) {
    let window = app.window.borrow().as_ref().map(|w| w.clone_strong());
    let host = match (&window, mode().as_str()) {
        (_, "off") | (None, _) => None,
        (Some(w), _) => app.hwnd().and_then(|h| {
            let wm = windowmgr::monitor(h.0 as i64).ok()?;
            let m = super::monitor_for(&wm.bounds)?;
            index_of(m.id)?; // a display without a panel yet
            Some(Host { hwnd: h.0 as i64, monitor: m.id, fullscreen: w.window().is_fullscreen() })
        }),
    };
    let before = ps(|p| std::mem::replace(&mut p.host, host.clone()));
    if before != host {
        if let Some(w) = &window {
            crate::ui::grid::refresh_crumbs(app, w);
        }
        push_content(app);
        if before.as_ref().map(|h| h.fullscreen) != host.as_ref().map(|h| h.fullscreen) {
            check_fullscreen(app);
        }
    }
    // Every time: the panel mode can change while the host stays the same.
    crate::ui::grid::set_hosted(app, host.is_some(), mode() == "autoHide");
    // A docked panel sits over the top of a fullscreen window: its content starts below.
    let inset = match (&host, &window) {
        (Some(h), Some(_)) if h.fullscreen && mode() == "always" => HEIGHT_DIP as f32,
        _ => 0.0,
    };
    if let Some(w) = &window {
        w.set_top_inset(inset);
    }
}

/// The breadcrumb the main window shows: Home ("") first.
pub fn set_crumbs(app: &App, crumbs: Vec<(String, String)>) {
    if ps(|p| p.crumbs == crumbs) {
        return;
    }
    ps(|p| p.crumbs = crumbs);
    let host = ps(|p| p.host.as_ref().map(|h| h.monitor));
    if let Some(i) = host.and_then(index_of) {
        push_one(app, i);
    }
}

/// While an item is dragged out of the grid: the hosted crumb under the
/// screen point, to highlight (and to drop on). None when not over one.
pub fn crumb_at(p: Point) -> Option<String> {
    let host = ps(|x| x.host.clone())?;
    let m = monitor_at(p)?;
    if m.id != host.monitor || p.y >= (m.bounds.top + bar_height(&m)) as f64 {
        return None;
    }
    let lx = ((p.x - m.bounds.left as f64) / m.scale) as f32;
    let i = with_panel(m.id, |x| x.crumb_geom.iter().position(|(cx, cw)| lx >= *cx && lx < cx + cw))??;
    ps(|x| x.crumbs.get(i).map(|c| c.0.clone()))
}

/// Highlights the crumb an item is dragged over (None: none).
pub fn set_drop_crumb(app: &App, p: Option<Point>) {
    let want = p.and_then(|p| {
        let id = crumb_at(p)?;
        let host = ps(|x| x.host.clone())?;
        let i = ps(|x| x.crumbs.iter().position(|c| c.0 == id))?;
        Some((host.monitor, i as i32))
    });
    if ps(|x| x.drop_crumb == want) {
        return;
    }
    ps(|x| x.drop_crumb = want);
    push_content(app);
}

/// Quitting: every panel gives its space back.
pub fn shutdown() {
    let gone: Vec<Panel> = ps(|p| {
        p.mode = "off".into();
        p.autohide = None;
        p.clock = None;
        p.recheck = None;
        p.focus_recheck = None;
        std::mem::take(&mut p.list)
    });
    for panel in gone {
        remove(panel);
    }
    appbar::remove_all();
}

/// Writes an empty appbars.json once nothing is docked (a clean quit).
pub fn forget_docked(app: &App) {
    let _ = launchpad_core::safe_file::write_file_atomic(&record_file(app), "[]");
}
