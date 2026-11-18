//! The drop-downs under a panel (lib/window-picker.js, lib/scenes-menu.js
//! in 1.2): the window picker, opened by a zone or the grid button when
//! there's no window to move, and the scenes menu. Each takes the focus (so
//! the keyboard works and a click elsewhere closes it); the window the user
//! was in, if it's another app's, is remembered and gets the focus back on
//! Esc or a pick. One menu at a time, existing only while open.
use super::{find_zone, grid, panel, st as org};
use crate::ui::shell::{self, Kind};
use crate::ui::{App, MenuRow, MenuWindow, Theme};
use crate::win::windowmgr::{self, events::Foreground, events::Kind as WinKind};
use launchpad_core::organizer::layouts as core_layouts;
use launchpad_core::organizer::{picker, OpenWindow};
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::HWND;

const PICKER_WIDTH: f64 = 320.0; // DIP
const SCENES_WIDTH: f64 = 280.0;
/// The card's transparent margin each side, for its shadow (organizer-windows.slint).
const MARGIN: f64 = 8.0;
const PICKER_EMPTY: &str = "No windows to move. Open an app, then try again.";
const SCENES_EMPTY: &str = "No scenes yet. Arrange your windows, then save them here.";

pub enum PickFor {
    Zone { layout: String, zone: String },
    Grid,
}

enum What {
    Picker { key: String, action: PickFor, entries: Vec<OpenWindow> },
    Scenes,
}

struct Menu {
    monitor: u32,
    what: What,
    win: MenuWindow,
    hwnd: HWND,
    previous: Option<Foreground>,
    /// Told apart from a menu opened since, for icons arriving late.
    seq: u32,
}

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
    static SEQ: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn ms<R>(f: impl FnOnce(&mut Option<Menu>) -> R) -> R {
    MENU.with_borrow_mut(f)
}

/// The window the user was in before the open menu took the focus.
pub fn previous() -> Option<Foreground> {
    ms(|m| m.as_ref().and_then(|m| m.previous.clone()))
}

/// `refocus`: give the user's window the focus back (Esc, a pick), but not
/// when the menu closed because they clicked elsewhere.
pub fn close(refocus: bool) {
    let Some(m) = ms(Option::take) else { return };
    // Before the menu hides, while Launchpad is still in front (see grid::close).
    if refocus {
        if let Some(p) = &m.previous {
            let _ = windowmgr::focus(p.hwnd);
        }
    }
    let _ = m.win.hide();
    drop(m);
    super::later(|app| panel::push_content(app));
}

/// What the menu takes the focus from: the window the user was in, if it's
/// another app's.
fn current_app() -> Option<Foreground> {
    previous().or_else(|| org(|s| s.focused.clone())).filter(|f| f.kind == WinKind::App)
}

/// Opens the picker under a panel (`x`: screen px, the clicked thing's left
/// edge). Asking again for the same thing on the same panel closes it.
pub fn open_picker(app: &Rc<App>, monitor: u32, x: i32, action: PickFor, heading: String) -> Result<(), String> {
    let key = match &action {
        PickFor::Zone { layout, zone } => format!("zone:{layout}:{zone}"),
        PickFor::Grid => "grid".into(),
    };
    if ms(|m| m.as_ref().is_some_and(|m| m.monitor == monitor && matches!(&m.what, What::Picker { key: k, .. } if *k == key))) {
        close(true);
        return Ok(());
    }
    let previous = current_app();
    grid::close(false);
    drop_menu();
    let listed: Vec<OpenWindow> = windowmgr::windows().iter().map(|w| w.open_window()).collect();
    let entries = picker::picker_entries(&listed);
    let rows: Vec<MenuRow> = entries
        .iter()
        .map(|e| MenuRow {
            id: e.hwnd.to_string().into(),
            title: e.title.as_str().into(),
            meta: "".into(),
            tag: if e.minimized { "minimized" } else { "" }.into(),
            icon: Image::default(),
            has_icon: true, // a blank square until the icon arrives, so rows don't shift
        })
        .collect();
    let exes: Vec<String> = entries.iter().map(|e| e.exe.clone()).collect();
    let seq = show(app, monitor, Anchor::Left(x), PICKER_WIDTH, &heading, rows, PICKER_EMPTY, false, previous, What::Picker { key, action, entries })?;
    fetch_icons(seq, exes);
    Ok(())
}

/// Opens the scenes menu under a panel's scenes button (`right`: the
/// button's right edge, in the panel's logical px). Again on the same panel
/// closes it; on another it moves there.
pub fn open_scenes(app: &Rc<App>, monitor: u32, right: f32) {
    if ms(|m| m.as_ref().is_some_and(|m| m.monitor == monitor && matches!(m.what, What::Scenes))) {
        close(true);
        return;
    }
    let previous = current_app();
    grid::close(false);
    drop_menu();
    let Some(m) = panel::monitor_of(monitor) else { return };
    let scenes = core_layouts::load(&app.data_dir).scenes;
    let rows: Vec<MenuRow> = scenes
        .iter()
        .map(|s| {
            let n = s.windows.len();
            MenuRow {
                id: s.id.as_str().into(),
                title: s.name.as_str().into(),
                meta: format!("{n} window{}", if n == 1 { "" } else { "s" }).into(),
                tag: if s.island_id.is_some() { "island" } else { "" }.into(),
                icon: Image::default(),
                has_icon: false,
            }
        })
        .collect();
    let x = m.bounds.left + (right as f64 * m.scale).round() as i32;
    if let Err(e) = show(app, monitor, Anchor::Right(x), SCENES_WIDTH, "", rows, SCENES_EMPTY, true, previous, What::Scenes) {
        panel::show_message(monitor, &e);
    }
}

enum Anchor {
    Left(i32),
    Right(i32),
}

/// Hidden, then dropped, without handing the focus anywhere.
fn drop_menu() {
    if let Some(m) = ms(Option::take) {
        let _ = m.win.hide();
    }
}

#[allow(clippy::too_many_arguments)]
fn show(
    app: &Rc<App>,
    monitor: u32,
    anchor: Anchor,
    width: f64,
    heading: &str,
    rows: Vec<MenuRow>,
    empty: &str,
    scenes: bool,
    previous: Option<Foreground>,
    what: What,
) -> Result<u32, String> {
    let m = panel::monitor_of(monitor).ok_or("Not a panel.")?;
    let win = shell::make(MenuWindow::new).map_err(|e| e.to_string())?;
    crate::ui::prefs::apply_theme(app, &win.global::<Theme>());
    let has_rows = !rows.is_empty();
    win.set_heading(heading.into());
    win.set_rows(ModelRc::new(VecModel::from(rows)));
    win.set_empty_text(empty.into());
    win.set_scenes(scenes);
    // The scenes menu starts on its first row; the picker on none, so Enter
    // can't move a window the user didn't pick.
    win.set_current(if scenes && has_rows { 0 } else { -1 });
    wire(&win);

    // Its top edge under the panel, kept on the display.
    let s = m.scale;
    let (outer_w, margin) = ((width + 2.0 * MARGIN) * s, MARGIN * s);
    let w = (width * s).round() as i32;
    let d = m.bounds;
    let left = match anchor {
        Anchor::Left(x) => x,
        Anchor::Right(x) => x - w,
    }
    .clamp(d.left, d.right - w);
    let top = panel::bottom_of(monitor);
    let h = ((win.get_content_height() as f64) * s).round().min((d.bottom - top) as f64 + margin).max(40.0 * s);
    let x = left - margin.round() as i32;
    let y = top - margin.round() as i32;
    win.window().set_position(slint::PhysicalPosition::new(x, y));
    win.window().set_size(slint::PhysicalSize::new(outer_w.round() as u32, h as u32));
    win.show().map_err(|e| e.to_string())?;
    win.invoke_take_focus();
    let seq = SEQ.get().wrapping_add(1);
    SEQ.set(seq);
    let rect = windows::Win32::Foundation::RECT { left: x, top: y, right: x + outer_w.round() as i32, bottom: y + h as i32 };
    shell::when_ready(&win, move |hwnd| {
        if !ms(|m| m.as_mut().filter(|m| m.seq == seq).map(|m| m.hwnd = hwnd).is_some()) {
            return;
        }
        shell::style(hwnd, Kind::Focusable);
        shell::place(hwnd, rect);
        super::close_on_blur(hwnd, || true, |h| {
            if ms(|m| m.as_ref().is_some_and(|m| m.hwnd == h)) {
                close(false);
            }
        });
        let _ = windowmgr::focus(hwnd.0 as i64);
    });
    ms(|slot| *slot = Some(Menu { monitor, what, win, hwnd: HWND::default(), previous, seq }));
    panel::push_content(app);
    Ok(seq)
}

/// Exe icons for the picker's rows, fetched off the UI thread.
fn fetch_icons(seq: u32, exes: Vec<String>) {
    std::thread::spawn(move || {
        let wic = crate::win::image::Wic::new().ok();
        let icons: Vec<Option<crate::win::image::Rgba>> =
            exes.iter().map(|e| wic.as_ref().and_then(|w| w.file_icon(std::path::Path::new(e), 32).ok())).collect();
        crate::ui::post(move |_| {
            let Some(win) = ms(|m| m.as_ref().filter(|m| m.seq == seq).map(|m| m.win.clone_strong())) else { return };
            let rows = win.get_rows();
            for (i, icon) in icons.into_iter().enumerate() {
                if let Some(mut r) = rows.row_data(i) {
                    match icon {
                        Some(rgba) => r.icon = crate::ui::images::to_image(rgba),
                        None => r.has_icon = false,
                    }
                    rows.set_row_data(i, r);
                }
            }
        });
    });
}

// Callbacks run inside the menu's own event handling: the work (which
// drops the menu) waits for `later`.
fn wire(win: &MenuWindow) {
    win.on_pick(|id| {
        let id = id.to_string();
        super::later(move |app| pick(app, &id));
    });
    win.on_save(|| super::later(save));
    win.on_manage(|| {
        super::later(|app| {
            if ms(|m| m.take()).map(|m| m.win.hide()).is_none() {
                return;
            }
            panel::push_content(app);
            app.show(true);
            super::dialogs::open_scenes(app);
        })
    });
    win.on_dismiss(|| super::later(|_| close(true)));
}

fn pick(app: &Rc<App>, id: &str) {
    let Some(m) = ms(Option::take) else { return };
    match &m.what {
        What::Scenes => {
            // Back to the user's window first, as 1.2 did, then the scene.
            if let Some(p) = &m.previous {
                let _ = windowmgr::focus(p.hwnd);
            }
            let _ = m.win.hide();
            let msg = super::scenes::apply(app, id).unwrap_or_else(|e| e);
            panel::show_message(m.monitor, &msg);
        }
        What::Picker { action, entries, .. } => {
            let Some(entry) = id.parse::<i64>().ok().and_then(|h| picker::find_entry(entries, h)).cloned() else {
                ms(|slot| *slot = Some(m)); // not one of its rows: leave it open
                return;
            };
            let target = Foreground {
                hwnd: entry.hwnd,
                kind: WinKind::App,
                visible: true,
                title: entry.title.clone(),
                exe: Some(entry.exe.clone()).filter(|e| !e.is_empty()),
                snappable: true,
            };
            let result = match action {
                PickFor::Grid => {
                    let _ = m.win.hide();
                    // The grid hands the window the focus when it closes.
                    grid::open(app, panel::monitor_of(m.monitor), Some(target))
                }
                PickFor::Zone { layout, zone } => {
                    let (layouts, gap) = org(|s| (s.layouts.clone(), s.gap));
                    let r = match (find_zone(&layouts, layout, zone), panel::monitor_of(m.monitor)) {
                        (Some((l, z)), Some(mon)) => super::place_in_zone(entry.hwnd, l, z, &mon, gap),
                        _ => Err(super::LAYOUT_GONE.into()),
                    };
                    // Before the picker hides, while Launchpad is still in front.
                    if r.is_ok() {
                        let _ = windowmgr::focus(entry.hwnd);
                    }
                    let _ = m.win.hide();
                    r
                }
            };
            if let Err(e) = result {
                panel::show_message(m.monitor, &e);
            }
        }
    }
    panel::push_content(app);
}

fn save(app: &Rc<App>) {
    let Some(m) = ms(Option::take) else { return };
    if let Some(p) = &m.previous {
        let _ = windowmgr::focus(p.hwnd);
    }
    let _ = m.win.hide();
    let msg = match super::scenes::save(app, None, None, None) {
        Ok(s) => {
            let n = s.windows.len();
            format!("Saved \"{}\" with {n} window{}. Rename it under Manage scenes.", s.name, if n == 1 { "" } else { "s" })
        }
        Err(e) => e,
    };
    panel::show_message(m.monitor, &msg);
    panel::push_content(app);
}
