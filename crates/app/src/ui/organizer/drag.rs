//! Drag to snap (organizer.js "drag to snap"): from a window's move loop
//! starting to its end, the cursor is polled (the dragged window has the
//! mouse captured) and a click-through overlay on the cursor's display
//! draws the layout strip and the hovered zone. The same overlay shows the
//! preview alone while a panel zone is hovered. Overlays hold a
//! screen-sized buffer, so each exists only while shown (Phase 0).
use super::{find_zone, monitor_at, panel, point_to_dip, rect, st as org, to_dip, zone_rect};
use crate::ui::shell::{self, Kind};
use crate::ui::{App, OverlayLabel, OverlayWindow, OverlayZone, Theme};
use crate::win::display::Monitor;
use launchpad_core::organizer::strip::{self, Strip, ZoneHit};
use launchpad_core::organizer::zones::Layout;
use launchpad_core::organizer::Rect;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Windows timers tick every ~15.6 ms; just under one tick gives ~64 Hz.
const CURSOR_POLL_MS: u64 = 10;

struct Drag {
    hwnd: i64,
    layouts: Vec<Layout>,
    gap: i32,
    _timer: Timer,
    monitor: Option<Monitor>,
    /// In the display's DIP space (core's strip works in DIP).
    strip: Option<Strip>,
    showing: bool,
    hovered: Option<ZoneHit>,
}

struct Overlay {
    monitor: u32,
    win: OverlayWindow,
}

#[derive(Default)]
struct DragState {
    drag: Option<Drag>,
    overlay: Option<Overlay>,
}

thread_local! {
    static DRAG: RefCell<DragState> = RefCell::new(DragState::default());
}

fn ds<R>(f: impl FnOnce(&mut DragState) -> R) -> R {
    DRAG.with_borrow_mut(f)
}

/// A window's move loop started (never sent for a resize or our own windows).
pub fn start(app: &Rc<App>, hwnd: i64) {
    cancel();
    if !super::snap_on_drag() {
        return;
    }
    destroy_overlays(); // a preview from the panel, if any
    panel::set_dragging(true);
    let (layouts, gap) = org(|s| (s.layouts.clone(), s.gap));
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(CURSOR_POLL_MS), || {
        if let Some(app) = super::with_app(Rc::clone) {
            track(&app);
        }
    });
    ds(|d| d.drag = Some(Drag { hwnd, layouts, gap, _timer: timer, monitor: None, strip: None, showing: false, hovered: None }));
    track(app);
}

fn track(app: &Rc<App>) {
    let p = super::cursor();
    let Some(m) = monitor_at(p) else { return };
    let changed_display = ds(|d| d.drag.as_ref().is_some_and(|g| g.monitor.as_ref().is_none_or(|o| o.id != m.id)));
    if changed_display {
        hide_overlay();
        ds(|d| {
            if let Some(g) = d.drag.as_mut() {
                g.strip = Some(strip::compute_strip(&g.layouts, to_dip(&m, rect(&m.work))));
                g.monitor = Some(m.clone());
                g.showing = false;
                g.hovered = None;
            }
        });
    }
    let update = ds(|d| {
        let g = d.drag.as_mut()?;
        let s = g.strip.as_ref()?;
        let pd = point_to_dip(&m, p);
        let showing = strip::should_show(s, to_dip(&m, rect(&m.work)), pd, g.showing);
        let hovered = if showing { strip::zone_at(s, pd) } else { None };
        if showing == g.showing && hovered == g.hovered {
            return None;
        }
        g.showing = showing;
        g.hovered = hovered;
        Some(showing)
    });
    match update {
        Some(true) => show_drag_overlay(app, &m),
        Some(false) => hide_overlay(),
        None => {}
    }
}

/// The move loop ended: drop on a zone places the window there; Esc, or a
/// drop anywhere else, leaves Windows' own handling.
pub fn finish(app: &Rc<App>, hwnd: i64, cancelled: bool) {
    if !ds(|d| d.drag.as_ref().is_some_and(|g| g.hwnd == hwnd)) {
        return;
    }
    track(app); // the last poll can be up to one interval old
    let Some(g) = ds(|d| d.drag.take()) else { return };
    cancel();
    let (Some(hit), Some(m)) = (g.hovered, g.monitor) else { return };
    if cancelled {
        return;
    }
    let Some((layout, zone)) = find_zone(&g.layouts, &hit.layout_id, &hit.zone_id) else { return };
    if let Err(e) = super::place_in_zone(hwnd, layout, zone, &m, g.gap) {
        super::notify(app, &e);
    }
}

pub fn cancel() {
    let had = ds(|d| d.drag.take().is_some());
    hide_overlay();
    if had {
        panel::set_dragging(false);
    }
}

/// Hovering a zone in a panel (None: off it): where the focused window
/// would go, full size, on that panel's display.
pub fn preview(app: &Rc<App>, monitor: u32, zone: Option<(String, String)>) {
    if ds(|d| d.drag.is_some()) {
        return;
    }
    let Some(m) = panel::monitor_of(monitor) else { return };
    let (layouts, gap) = org(|s| (s.layouts.clone(), s.gap));
    let Some((layout, z)) = zone.as_ref().and_then(|(l, z)| find_zone(&layouts, l, z)) else {
        hide_overlay();
        return;
    };
    let r = zone_rect(layout, z, &m, gap);
    let Some(win) = overlay_for(app, &m) else { return };
    let s = m.scale as f32;
    win.set_has_strip(false);
    win.set_has_preview(true);
    win.set_preview_x((r.left - m.work.left) as f32 / s);
    win.set_preview_y((r.top - m.work.top) as f32 / s);
    win.set_preview_w((r.right - r.left) as f32 / s);
    win.set_preview_h((r.bottom - r.top) as f32 / s);
}

fn show_drag_overlay(app: &Rc<App>, m: &Monitor) {
    let Some((strip, hovered, layouts, gap)) =
        ds(|d| d.drag.as_ref().and_then(|g| g.strip.clone().map(|s| (s, g.hovered.clone(), g.layouts.clone(), g.gap))))
    else {
        return;
    };
    let Some(win) = overlay_for(app, m) else { return };
    // Strip rects are in the display's DIP space: relative to the work area
    // in DIP is exactly the overlay's logical coordinates.
    let wa = to_dip(m, rect(&m.work));
    let rel = |r: Rect| (r.x - wa.x, r.y - wa.y, r.width, r.height);
    let (x, y, w, h) = rel(strip.rect);
    win.set_has_strip(true);
    win.set_strip_x(x as f32);
    win.set_strip_y(y as f32);
    win.set_strip_w(w as f32);
    win.set_strip_h(h as f32);
    let mut zones = Vec::new();
    let mut labels = Vec::new();
    for item in &strip.items {
        for z in &item.zones {
            let (x, y, w, h) = rel(z.rect);
            zones.push(OverlayZone {
                layout_id: item.layout_id.as_str().into(),
                zone_id: z.zone_id.as_str().into(),
                x: x as f32,
                y: y as f32,
                w: w as f32,
                h: h as f32,
            });
        }
        let (x, y, w, h) = rel(item.thumb);
        labels.push(OverlayLabel { layout_id: item.layout_id.as_str().into(), name: item.name.as_str().into(), x: x as f32, y: (y + h + 4.0) as f32, w: w as f32 });
    }
    win.set_zones(ModelRc::new(VecModel::from(zones)));
    win.set_labels(ModelRc::new(VecModel::from(labels)));
    win.set_hovered_layout(hovered.as_ref().map(|h| h.layout_id.as_str()).unwrap_or("").into());
    win.set_hovered_zone(hovered.as_ref().map(|h| h.zone_id.as_str()).unwrap_or("").into());
    let preview = hovered.as_ref().and_then(|h| find_zone(&layouts, &h.layout_id, &h.zone_id));
    win.set_has_preview(preview.is_some());
    if let Some((layout, zone)) = preview {
        let r = zone_rect(layout, zone, m, gap);
        let s = m.scale as f32;
        win.set_preview_x((r.left - m.work.left) as f32 / s);
        win.set_preview_y((r.top - m.work.top) as f32 / s);
        win.set_preview_w((r.right - r.left) as f32 / s);
        win.set_preview_h((r.bottom - r.top) as f32 / s);
    }
}

/// The overlay on `m`, made (and shown) if needed; another display's goes.
fn overlay_for(app: &Rc<App>, m: &Monitor) -> Option<OverlayWindow> {
    if let Some(w) = ds(|d| d.overlay.as_ref().filter(|o| o.monitor == m.id).map(|o| o.win.clone_strong())) {
        return Some(w);
    }
    hide_overlay();
    let win = shell::make(OverlayWindow::new).ok()?;
    crate::ui::prefs::apply_theme(app, &win.global::<Theme>());
    let wa = m.work;
    win.window().set_position(slint::PhysicalPosition::new(wa.left, wa.top));
    win.window().set_size(slint::PhysicalSize::new((wa.right - wa.left) as u32, (wa.bottom - wa.top) as u32));
    win.show().ok()?;
    shell::when_ready(&win, move |h| {
        shell::style(h, Kind::ClickThrough);
        shell::place(h, wa);
    });
    ds(|d| d.overlay = Some(Overlay { monitor: m.id, win: win.clone_strong() }));
    Some(win)
}

/// Hidden, then dropped: dropping a shown window would leak its buffer.
fn hide_overlay() {
    if let Some(o) = ds(|d| d.overlay.take()) {
        let _ = o.win.hide();
    }
}

pub fn destroy_overlays() {
    hide_overlay();
}
