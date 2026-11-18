//! The grid placer (lib/grid.js, renderer/grid.js in 1.2): over one display's
//! work area, pick a block of cells for a window, gTile-style. It opens for
//! the focused window (hotkey: on its display; a panel's grid button: on
//! that panel's display), or one chosen in the picker. It takes the focus
//! (it needs the keyboard), so the window it moves is remembered and gets
//! the focus back when it closes. Like the overlays, the window exists only
//! while open.
use super::{menus, panel, st as org, zone_rect, NOTHING_TO_MOVE};
use crate::ui::shell::{self, Kind};
use crate::ui::{App, GridPlacerWindow, Theme};
use crate::win::display::Monitor;
use crate::win::windowmgr::{self, events::Foreground};
use launchpad_core::organizer::cells::{self, Block};
use launchpad_core::organizer::zones::{Layout, Zone};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows::Win32::Foundation::HWND;

// Cells across and down: 6 x 4, or 4 x 6 on a display taller than it's wide.
const LONG_SIDE: i32 = 6;
const SHORT_SIDE: i32 = 4;
const HINT: &str = "Drag across cells, or click two corners · Arrows move · Shift+arrows resize · Enter places · Esc cancels";
const CORNER_HINT: &str = "Now click the opposite corner.";

struct Grid {
    target: Foreground,
    monitor: Monitor,
    cols: i32,
    rows: i32,
    gap: i32,
    win: GridPlacerWindow,
    hwnd: HWND,
    /// The first corner, while dragging or after a click on one cell.
    anchor: Option<(i32, i32)>,
    dragging: bool,
}

thread_local! {
    static GRID: RefCell<Option<Grid>> = const { RefCell::new(None) };
    // Placing a maximized window restores it, which takes the focus: that
    // isn't the user clicking elsewhere. A Cell, read from the window proc.
    static PLACING: Cell<bool> = const { Cell::new(false) };
}

fn gs<R>(f: impl FnOnce(&mut Option<Grid>) -> R) -> R {
    GRID.with_borrow_mut(f)
}

/// The window the open grid moves.
pub fn target() -> Option<Foreground> {
    gs(|g| g.as_ref().map(|g| g.target.clone()))
}

pub fn is_open() -> bool {
    gs(|g| g.is_some())
}

/// `display`: where to open it, None for the display the window is on.
/// Asking again where it's already open closes it. `chosen`: the window
/// picked in the picker, instead of the focused one.
pub fn open(app: &Rc<App>, display: Option<Monitor>, chosen: Option<Foreground>) -> Result<(), String> {
    let open_on = gs(|g| g.as_ref().map(|g| g.monitor.id));
    if chosen.is_none() && open_on.is_some_and(|id| display.as_ref().is_none_or(|d| d.id == id)) {
        close(true);
        return Ok(());
    }
    let target = chosen.or_else(|| {
        target().or_else(|| menus::previous().or_else(|| org(|s| s.focused.clone())).filter(|t| t.snappable))
    });
    let Some(target) = target else { return Err(NOTHING_TO_MOVE.into()) };
    menus::close(false);
    let wm = windowmgr::monitor(target.hwnd).map_err(super::fail_message)?;
    let Some(m) = display.or_else(|| super::monitor_for(&wm.bounds)) else { return Err(NOTHING_TO_MOVE.into()) };
    hide_window();

    let wa = m.work;
    let (aw, ah) = (wa.right - wa.left, wa.bottom - wa.top);
    let (cols, rows) = if ah > aw { (SHORT_SIDE, LONG_SIDE) } else { (LONG_SIDE, SHORT_SIDE) };
    let gap = org(|s| s.gap);
    // Starts where the window already is, so Enter alone lines it up with the grid.
    let f = wm.frame;
    let frame = launchpad_core::organizer::Rect::new(
        (f.left - wa.left) as f64,
        (f.top - wa.top) as f64,
        (f.right - f.left) as f64,
        (f.bottom - f.top) as f64,
    );
    let block = (!wm.minimized)
        .then(|| cells::block_from_rect(frame, aw as f64, ah as f64, cols, rows))
        .flatten()
        .unwrap_or_else(|| cells::default_block(cols, rows));

    let win = shell::make(GridPlacerWindow::new).map_err(|e| e.to_string())?;
    crate::ui::prefs::apply_theme(app, &win.global::<Theme>());
    win.set_cols(cols);
    win.set_rows(rows);
    win.set_gap(gap as f32);
    win.set_target_title(target.title.as_str().into());
    win.set_target_icon(panel::icon_for(app, target.exe.as_deref()).unwrap_or_default());
    set_block(&win, block);
    win.set_hint(HINT.into());
    wire(&win);
    win.window().set_position(slint::PhysicalPosition::new(wa.left, wa.top));
    win.window().set_size(slint::PhysicalSize::new(aw as u32, ah as u32));
    win.show().map_err(|e| e.to_string())?;
    win.invoke_take_focus();
    shell::when_ready(&win, move |hwnd| {
        if !gs(|g| g.as_mut().map(|g| g.hwnd = hwnd).is_some()) {
            return;
        }
        shell::style(hwnd, Kind::Focusable);
        shell::place(hwnd, wa);
        super::close_on_blur(hwnd, || !PLACING.get(), |h| {
            if gs(|g| g.as_ref().is_some_and(|g| g.hwnd == h)) {
                close(false);
            }
        });
        let _ = windowmgr::focus(hwnd.0 as i64);
    });
    gs(|g| *g = Some(Grid { target, monitor: m, cols, rows, gap, win, hwnd: HWND::default(), anchor: None, dragging: false }));
    panel::push_content(app);
    Ok(())
}

/// `refocus`: give the window the focus back (Esc, or after placing it), but
/// not when the grid closed because the user clicked something else.
pub fn close(refocus: bool) {
    let Some(g) = gs(Option::take) else { return };
    // Before the grid hides, while Launchpad is still in front: Windows only
    // lets the foreground process bring another window to the front.
    if refocus {
        let _ = windowmgr::focus(g.target.hwnd);
    }
    let _ = g.win.hide();
    drop(g);
    super::later(|app| panel::push_content(app));
}

fn hide_window() {
    if let Some(g) = gs(Option::take) {
        let _ = g.win.hide();
    }
}

fn set_block(win: &GridPlacerWindow, b: Block) {
    win.set_bx(b.x);
    win.set_by(b.y);
    win.set_bw(b.w);
    win.set_bh(b.h);
}

fn block_of(win: &GridPlacerWindow) -> Block {
    Block { x: win.get_bx(), y: win.get_by(), w: win.get_bw(), h: win.get_bh() }
}

fn hint(win: &GridPlacerWindow, text: &str, error: bool) {
    win.set_hint(text.into());
    win.set_hint_error(error);
}

// The window's callbacks run inside its own event handling; anything that
// may close it waits for `later`.
fn wire(win: &GridPlacerWindow) {
    win.on_cell_down(|x, y| {
        let Some((win, second)) = gs(|g| {
            let g = g.as_mut()?;
            let second = g.anchor.filter(|_| !g.dragging);
            if second.is_none() {
                g.anchor = Some((x, y));
                g.dragging = true;
            }
            Some((g.win.clone_strong(), second))
        }) else {
            return;
        };
        match second {
            Some(a) => {
                set_block(&win, cells::cell_span(a, (x, y))); // the second corner
                super::later(|_| place());
            }
            None => {
                set_block(&win, cells::cell_span((x, y), (x, y)));
                hint(&win, HINT, false);
            }
        }
    });
    win.on_cell_move(|x, y| {
        if let Some((win, a)) = gs(|g| g.as_ref().and_then(|g| g.anchor.map(|a| (g.win.clone_strong(), a)))) {
            set_block(&win, cells::cell_span(a, (x, y)));
        }
    });
    win.on_cell_up(|x, y| {
        let Some((win, a)) = gs(|g| {
            let g = g.as_mut().filter(|g| g.dragging)?;
            g.dragging = false;
            Some((g.win.clone_strong(), g.anchor?))
        }) else {
            return;
        };
        if a == (x, y) {
            hint(&win, CORNER_HINT, false); // a click, not a drag: wait for the other corner
            return;
        }
        set_block(&win, cells::cell_span(a, (x, y)));
        super::later(|_| place());
    });
    win.on_cancel(|| super::later(|_| close(true)));
    win.on_key(|key, shift| {
        let Some((win, cols, rows)) = gs(|g| g.as_mut().map(|g| (g.win.clone_strong(), g.cols, g.rows))) else { return false };
        let (dx, dy) = match key.as_str() {
            "enter" => {
                super::later(|_| place());
                return true;
            }
            "left" => (-1, 0),
            "right" => (1, 0),
            "up" => (0, -1),
            "down" => (0, 1),
            _ => return false,
        };
        // The keyboard takes over from a half-made mouse selection.
        gs(|g| {
            if let Some(g) = g.as_mut() {
                g.anchor = None;
                g.dragging = false;
            }
        });
        hint(&win, HINT, false);
        let b = block_of(&win);
        set_block(&win, if shift { cells::resize_block(b, dx, dy, cols, rows) } else { cells::move_block(b, dx, dy, cols, rows) });
        true
    });
}

/// Moves the window to the selected cells; the grid closes once it's there.
fn place() {
    let Some((win, hwnd, m, cols, rows, gap)) =
        gs(|g| g.as_mut().map(|g| {
            g.anchor = None;
            g.dragging = false;
            (g.win.clone_strong(), g.target.hwnd, g.monitor.clone(), g.cols, g.rows, g.gap)
        }))
    else {
        return;
    };
    let b = block_of(&win);
    let zone = Zone::new("cells", "", b.x, b.y, b.w, b.h);
    let layout = Layout { id: "grid".into(), name: "Grid".into(), cols, rows, zones: vec![zone.clone()], ..Default::default() };
    PLACING.set(true);
    let r = windowmgr::place(hwnd, zone_rect(&layout, &zone, &m, gap), false);
    PLACING.set(false);
    match r {
        Ok(_) => close(true),
        Err(f) => hint(&win, &super::fail_message(f), true),
    }
}
