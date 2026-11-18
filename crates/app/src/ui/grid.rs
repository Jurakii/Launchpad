//! The top bar and the grid (ui/grid.slint): the open level's tiles and
//! islands, breadcrumb, drag to move or reposition, marquee and Ctrl/Shift
//! selection, island resize, keyboard, context menus, item actions and files
//! dropped from Explorer. Ported from renderer.js; the rules are in
//! grid_logic.rs, the OLE drop target in grid_drop.rs.
use super::{later, modals, post, settings, with, App, GridCrumb, GridItem, GridMenuEntry, GridMini, GridState, MainWindow, Popouts};
use crate::win::{self, image::Wic, launch, power, services::Thumbs};
use launchpad_core::file_store::{self, StoreOptions, StoreResult};
use launchpad_core::items::{self, Shortcut};
use launchpad_core::store::{self, Entry, READ_ONLY_MESSAGE};
use launchpad_core::{layout, time, Result};
use slint::{Color, ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[path = "grid_drop.rs"]
pub(crate) mod drop;
#[path = "grid_logic.rs"]
mod logic;
use logic::{Rect, CARD_MARGIN, CELL, TILE};

const ISLAND_COLOR: &str = "#5b8cff"; // ISLAND_COLORS[0]

struct Drag {
    ids: Vec<String>,
    id: String,
    start: (f64, f64),
}

struct Marquee {
    start: (f64, f64),
    base: Vec<String>,
    additive: bool,
    moved: bool,
}

struct Resize {
    id: String,
    start: (f64, f64),
    fp: (f64, f64),
    snap: (f64, f64),
}

/// Grid state that outlives the window (it's dropped on hide).
#[derive(Default)]
struct State {
    /// The open island; None is Home.
    folder: Option<String>,
    /// The level last drawn, to clear the selection when it changes.
    shown: Option<Option<String>>,
    /// Multi-selection, in the order things were added (a JS Set).
    selected: Vec<String>,
    /// Keyboard selection: an index into `level`.
    focus: i32,
    /// The open level as drawn: id, box, island.
    level: Vec<(String, Rect, bool)>,
    drag: Option<Drag>,
    marquee: Option<Marquee>,
    resize: Option<Resize>,
    /// Whose menu was last filled ("" = the canvas), and where (canvas px).
    menu: (String, (f64, f64)),
    /// Thumbnails asked for this session (renderer.js thumbRequests).
    thumbs: HashSet<String>,
    /// The HWND our drop target is on.
    drop_hwnd: isize,
    /// The organizer's panel carries the top bar: (hosted, auto-hiding).
    hosted: (bool, bool),
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State { focus: -1, ..Default::default() });
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    S.with(|s| f(&mut s.borrow_mut()))
}

fn win(app: &App) -> Option<MainWindow> {
    app.window.borrow().as_ref().map(|w| w.clone_strong())
}

fn find<'a>(apps: &'a [Entry], id: &str) -> Option<&'a Entry> {
    apps.iter().find(|a| a.str("id") == Some(id))
}

fn is_island(e: &Entry) -> bool {
    e.str("kind") == Some("island")
}

/// The open island, for other parts (search opening an island). None = Home.
pub fn open_island(app: &App, id: Option<String>) {
    st(|s| s.folder = id.filter(|i| !i.is_empty()));
    if let Some(w) = win(app) {
        refresh(app, &w);
    }
}

/// The open island ("" none), for the organizer panel's toolbar.
pub fn current_island() -> Option<String> {
    st(|s| s.folder.clone())
}

/// The island's right-click menu, opened from the organizer panel's breadcrumb.
pub fn open_island_menu(app: &Rc<App>, id: &str) {
    let id = id.to_owned();
    // After the window has laid out (it may have just been made).
    later(move |app| {
        let Some(w) = win(app) else { return };
        let g = w.global::<GridState>();
        let at = (g.get_grid_x() as f64 + 24.0, g.get_grid_y() as f64 + 8.0);
        fill_menu(app, &id, at);
        g.set_menu_x(at.0 as f32);
        g.set_menu_y(at.1 as f32);
        g.set_menu_tick(g.get_menu_tick() + 1);
    });
    let _ = app;
}

pub fn open_media(app: &Rc<App>) {
    super::media::open(app);
}

/// From the organizer: whether its panel carries the top bar.
pub fn set_hosted(app: &App, hosted: bool, autohide: bool) {
    st(|s| s.hosted = (hosted, autohide));
    if let Some(w) = win(app) {
        let g = w.global::<GridState>();
        g.set_hosted(hosted);
        g.set_hosted_autohide(hosted && autohide);
    }
}

/// The breadcrumb as the organizer panel shows it: Home ("") first.
pub fn refresh_crumbs(app: &App, _w: &MainWindow) {
    let folder = st(|s| s.folder.clone());
    let chain = logic::ancestors(&app.apps.borrow(), folder.as_deref());
    let crumbs = std::iter::once((String::new(), "Home".to_owned())).chain(chain).collect();
    super::organizer::panel::set_crumbs(app, crumbs);
}

/// `change` for an edit with nothing to return.
fn apply(app: &Rc<App>, f: impl FnOnce(&mut Vec<Entry>)) {
    change(app, |apps| {
        f(apps);
        Ok(())
    });
}

/// refreshFromServer's failure path: redraw from the unchanged list (an
/// optimistic drag snaps back) and say why.
fn change<R>(app: &Rc<App>, f: impl FnOnce(&mut Vec<Entry>) -> Result<R>) -> Option<R> {
    match app.mutate(f) {
        Ok(r) => Some(r),
        Err(e) => {
            app.refresh();
            modals::alert(app, &format!("Launchpad couldn't do that:\n{e}"));
            None
        }
    }
}

// ---------- drawing ----------

fn color_of(e: &Entry) -> Color {
    let c = e.str("color").filter(|c| !c.is_empty()).unwrap_or(ISLAND_COLOR);
    super::prefs::parse_hex(c).or_else(|| super::prefs::parse_hex(ISLAND_COLOR)).unwrap_or_default()
}

/// iconNode: an image, or which of 1.2's drawn glyphs (see ItemIcon).
fn icon_of(app: &App, e: &Entry, px: u32) -> (Image, i32, bool) {
    if is_island(e) {
        return (Image::default(), 1, false);
    }
    let stored = e.str("type") == Some("stored");
    if stored && e.str("storedKind") == Some("folder") {
        return (Image::default(), 2, false);
    }
    if e.str("special") == Some("recycleBin") && !e.truthy("iconIsCustom") {
        return (Image::default(), 3, false);
    }
    let thumb = stored && e.str("thumbState") == Some("ready");
    let size = if thumb { px * 3 / 2 } else { px };
    match e.str("iconPath").filter(|p| !p.is_empty()).and_then(|p| app.images.icon(Path::new(p), size)) {
        Some(img) => (img, 0, thumb),
        None => (Image::default(), 4, false),
    }
}

/// decorate's `missing`.
fn is_missing(e: &Entry) -> bool {
    // shortcut: checked on the UI thread on every refresh, as 1.2's main did; a dead network path would stall it.
    e.str("kind") == Some("app")
        && matches!(e.str("type"), Some("path" | "script" | "stored"))
        && e.str("target").is_none_or(|t| t.is_empty() || !Path::new(t).exists())
}

/// tileTitle: the tooltip, a stored item's info card.
fn tile_title(e: &Entry, missing: bool) -> String {
    let name = e.str("name").unwrap_or("");
    if e.str("type") == Some("stored") {
        let mut lines = vec![name.to_owned()];
        if missing {
            lines.push("⚠ The stored copy is missing. Remove this item, or drop the file again.".into());
        }
        let kind = if e.str("storedKind") == Some("folder") { "folder" } else { "file" };
        let size = e.num("size").map(|n| format!(", {}", file_store::format_size(n))).unwrap_or_default();
        lines.push(format!("Stored {kind}{size}"));
        if let Some(o) = e.str("originalPath").filter(|o| !o.is_empty()) {
            lines.push(format!("From: {o}"));
        }
        if let Some(at) = e.num("createdAt").filter(|n| *n != 0.0) {
            lines.push(format!("Added: {}", logic::us_date_time(local_ms(at as i64))));
        }
        return lines.join("\n");
    }
    if missing {
        format!("{name}\n⚠ Target not found:\n{}", e.str("target").unwrap_or(""))
    } else {
        name.to_owned()
    }
}

fn local_ms(utc_ms: i64) -> i64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    let t = (utc_ms + 11_644_473_600_000) as u64 * 10_000;
    let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
    let mut local = FILETIME::default();
    if unsafe { FileTimeToLocalFileTime(&ft, &mut local) }.is_err() {
        return utc_ms;
    }
    ((((local.dwHighDateTime as u64) << 32 | local.dwLowDateTime as u64) / 10_000) as i64) - 11_644_473_600_000
}

fn minis(app: &App, apps: &[Entry], island: &Entry, l: &logic::CardLayout, px: u32) -> Vec<GridMini> {
    let id = island.str("id").unwrap_or("");
    let mut contents: Vec<&Entry> = apps.iter().filter(|a| a.str("parentId") == Some(id)).collect();
    contents.sort_by(|a, b| logic::name_order(a, b));
    let shown = l.shown(contents.len());
    let mut out: Vec<GridMini> = contents[..shown]
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (icon, glyph, _) = icon_of(app, c, px);
            let (x, y) = l.pos(i);
            GridMini {
                id: c.str("id").unwrap_or("").into(),
                name: c.str("name").unwrap_or("").into(),
                icon,
                glyph,
                color: color_of(c),
                x: x as f32,
                y: y as f32,
            }
        })
        .collect();
    if contents.len() > shown {
        let (x, y) = l.pos(shown);
        out.push(GridMini { name: format!("+{}", contents.len() - shown).into(), glyph: 5, x: x as f32, y: y as f32, ..Default::default() });
    }
    out
}

fn items_model(w: &MainWindow) -> Rc<VecModel<GridItem>> {
    // A fresh model per refresh, as 1.2 redrew the canvas; drags and the marquee update rows in place.
    let v = Rc::new(VecModel::<GridItem>::default());
    w.global::<GridState>().set_items(ModelRc::from(v.clone()));
    v
}

/// Re-marks selected / dragged rows in place (marquee and drags move often).
fn update_rows(w: &MainWindow) {
    let m = w.global::<GridState>().get_items();
    let (sel, dragged) = st(|s| (s.selected.clone(), s.drag.as_ref().map(|d| d.ids.clone()).unwrap_or_default()));
    for i in 0..m.row_count() {
        let Some(mut row) = m.row_data(i) else { continue };
        let (a, b) = (sel.iter().any(|s| row.id == *s), dragged.iter().any(|d| row.id == *d));
        if row.selected != a || row.dragged != b {
            row.selected = a;
            row.dragged = b;
            m.set_row_data(i, row);
        }
    }
}

/// Pushes the open level into the window.
pub fn refresh(app: &App, w: &MainWindow) {
    ensure_drop_target(w);
    let g = w.global::<GridState>();
    let scale = w.window().scale_factor();
    let px = |logical: f64| (logical * scale as f64).round() as u32;
    let apps = app.apps.borrow();
    let mut pending = Vec::new();

    let folder = st(|s| {
        if s.folder.as_deref().is_some_and(|f| !find(&apps, f).is_some_and(is_island)) {
            s.folder = None; // the island we stood in is gone
        }
        if s.shown.as_ref() != Some(&s.folder) {
            s.selected.clear();
            s.shown = Some(s.folder.clone());
        }
        s.selected.retain(|id| find(&apps, id).is_some());
        s.focus = -1;
        s.folder.clone()
    });
    let at_level = |a: &&Entry| a.str("parentId").filter(|p| !p.is_empty()) == folder.as_deref();
    let mut level: Vec<&Entry> = apps.iter().filter(at_level).collect();
    level.sort_by(|a, b| {
        let (ay, by) = (logic::cell_num(a, "gridY"), logic::cell_num(b, "gridY"));
        ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal).then(
            logic::cell_num(a, "gridX").partial_cmp(&logic::cell_num(b, "gridX")).unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    let selected = st(|s| s.selected.clone());
    let (mut max_col, mut max_row) = (0f64, 0f64);
    let mut placed = Vec::new();
    let mut rows = Vec::new();
    for e in &level {
        let id = e.str("id").unwrap_or("").to_owned();
        let (gx, gy) = (logic::cell_num(e, "gridX"), logic::cell_num(e, "gridY"));
        let fp = layout::footprint_of(e);
        max_col = max_col.max(gx + fp.w);
        max_row = max_row.max(gy + fp.h);
        let card = is_island(e) && e.get("collapsed") == Some(&serde_json::Value::Bool(false));
        let (w_px, h_px) = if card { (fp.w * CELL - CARD_MARGIN, fp.h * CELL - CARD_MARGIN) } else { (TILE, TILE) };
        placed.push((id.clone(), (gx * CELL, gy * CELL, w_px, h_px), is_island(e)));
        let missing = is_missing(e);
        if e.str("type") == Some("stored") && e.str("thumbState") == Some("pending") && !missing {
            pending.push(id.clone());
        }
        let (icon, glyph, thumb) = icon_of(app, e, px(48.0));
        let show_names = e.truthy("showNames");
        let (icon_size, mini_list, empty_card) = if card {
            let l = logic::card_layout(fp.w, fp.h, show_names);
            let m = minis(app, &apps, e, &l, px(l.icon * 0.64));
            (l.icon, m.clone(), m.is_empty())
        } else {
            (0.0, Vec::new(), false)
        };
        rows.push(GridItem {
            id: id.as_str().into(),
            card,
            name: e.str("name").unwrap_or("").into(),
            tip: tile_title(e, missing).into(),
            icon,
            glyph,
            color: color_of(e),
            x: (gx * CELL) as f32,
            y: (gy * CELL) as f32,
            w: w_px as f32,
            h: h_px as f32,
            missing,
            stored: e.str("type") == Some("stored"),
            thumb,
            selected: selected.contains(&id),
            dragged: false,
            show_names,
            icon_size: icon_size as f32,
            minis: ModelRc::new(VecModel::from(mini_list)),
            empty_card,
        });
    }
    st(|s| s.level = placed);
    items_model(w).set_vec(rows);

    let chain = logic::ancestors(&apps, folder.as_deref());
    let crumbs: Vec<GridCrumb> = std::iter::once(GridCrumb { id: "".into(), name: "Home".into() })
        .chain(chain.iter().map(|(id, name)| GridCrumb { id: id.into(), name: name.into() }))
        .collect();
    g.set_crumbs(ModelRc::new(VecModel::from(crumbs)));
    let (hosted, autohide) = st(|s| s.hosted);
    g.set_hosted(hosted);
    g.set_hosted_autohide(hosted && autohide);
    g.set_in_island(!chain.is_empty());
    g.set_empty(level.is_empty());
    g.set_empty_title(if folder.is_some() { "This island is empty." } else { "No apps yet." }.into());
    g.set_content_w((max_col * CELL + 24.0) as f32);
    g.set_content_h((max_row * CELL + 24.0) as f32);
    g.set_focused(-1);
    g.set_fullscreen(w.window().is_fullscreen());
    drop(apps);
    refresh_crumbs(app, w);

    for id in pending {
        if st(|s| s.thumbs.insert(id.clone())) {
            restore_thumb(app, id, false);
        }
    }
}

// ---------- wiring ----------

/// Wires the global's callbacks on a newly made window.
pub fn bind(_app: &Rc<App>, w: &MainWindow) {
    // A new window: its HWND (maybe a reused value) needs our drop target.
    st(|s| s.drop_hwnd = 0);
    let g = w.global::<GridState>();
    g.on_crumb(|id| with(|app| open_island(app, Some(id.to_string()))).unwrap_or(()));
    g.on_back(|| {
        with(|app| {
            let up = app.apps.borrow();
            let parent = st(|s| s.folder.clone()).and_then(|f| find(&up, &f).and_then(|e| e.str("parentId").map(str::to_owned)));
            drop(up);
            open_island(app, parent);
        });
    });
    g.on_settings(|| {
        with(|app| settings::open(app, None));
    });
    g.on_new_island(|| {
        with(|app| modals::new_island(app, st(|s| s.folder.clone()), None));
    });
    g.on_add_app(|| {
        with(|app| modals::add_app(app, st(|s| s.folder.clone()), None));
    });
    g.on_media(|| later(open_media));
    g.on_layouts(|| later(super::organizer::dialogs::open_layouts));
    g.on_toggle_fullscreen(|| {
        with(|app| {
            if let Some(w) = win(app) {
                let on = !w.window().is_fullscreen();
                w.window().set_fullscreen(on);
                w.global::<GridState>().set_fullscreen(on);
            }
            later(super::organizer::panel::update_host);
        });
    });
    g.on_resized(|| {
        with(|app| {
            if let Some(w) = win(app) {
                w.global::<GridState>().set_fullscreen(w.window().is_fullscreen());
            }
        });
    });
    g.on_tap(|id, toggle| {
        with(|app| tap(app, &id, toggle));
    });
    g.on_edit(|id| {
        with(|app| edit(app, &id));
    });
    g.on_collapse(|id| {
        with(|app| apply(app, |apps| items::set_collapsed(apps, &id, true)));
    });
    g.on_drag_start(|id, x, y| {
        with(|app| drag_start(app, &id, (x as f64, y as f64)));
    });
    g.on_drag_move(|x, y| {
        with(|app| drag_move(app, (x as f64, y as f64)));
    });
    g.on_drag_end(|x, y| {
        with(|app| drag_end(app, (x as f64, y as f64)));
    });
    g.on_marquee_start(|x, y, additive| {
        with(|app| marquee_start(app, (x as f64, y as f64), additive));
    });
    g.on_marquee_move(|x, y| {
        with(|app| marquee_move(app, (x as f64, y as f64)));
    });
    g.on_marquee_end(|| {
        with(marquee_end);
    });
    g.on_resize_start(|id, x, y| {
        with(|app| resize_start(app, &id, (x as f64, y as f64)));
    });
    g.on_resize_move(|x, y| {
        with(|app| resize_move(app, (x as f64, y as f64)));
    });
    g.on_resize_end(|| {
        with(resize_end);
    });
    g.on_menu_for(|id, x, y| {
        with(|app| fill_menu(app, &id, (x as f64, y as f64)));
    });
    g.on_menu_pick(|action| {
        // After the menu's own callback has returned: actions open dialogs.
        let action = action.to_string();
        later(move |app| menu_pick(app, &action));
    });
    g.on_key(|text, ctrl, alt, shift| with(|app| key(app, &text, ctrl, alt, shift)).unwrap_or(false));
    // The HWND exists only once the window is shown, right after this.
    later(|app| {
        if let Some(w) = win(app) {
            ensure_drop_target(&w);
        }
    });
    // Test hook (debug builds): LAUNCHPAD_DEBUG_DROP="x,y,alt|path|path…" acts as an
    // Explorer drop at client px x,y, since a test can't drag from Explorer.
    #[cfg(debug_assertions)]
    if let Ok(spec) = std::env::var("LAUNCHPAD_DEBUG_DROP") {
        let mut parts = spec.split('|');
        let head: Vec<i32> = parts.next().unwrap_or("").split(',').filter_map(|n| n.parse().ok()).collect();
        let paths: Vec<String> = parts.map(str::to_owned).collect();
        if let [x, y, alt] = head[..] {
            slint::Timer::single_shot(std::time::Duration::from_secs(1), move || on_files(paths, alt != 0, x, y));
        }
    }
}

fn canvas_point(w: &MainWindow, (x, y): (f64, f64)) -> (f64, f64) {
    let g = w.global::<GridState>();
    (x - g.get_origin_x() as f64, y - g.get_origin_y() as f64)
}

/// Whether a window point is over the grid's viewport (1.2's grid element).
fn in_grid(w: &MainWindow, (x, y): (f64, f64)) -> bool {
    let g = w.global::<GridState>();
    let r = (g.get_grid_x() as f64, g.get_grid_y() as f64, g.get_view_w() as f64, g.get_view_h() as f64);
    logic::contains(r, x, y)
}

fn level_ids() -> Vec<String> {
    st(|s| s.level.iter().map(|l| l.0.clone()).collect())
}

fn set_selected(app: &App, sel: Vec<String>) {
    st(|s| s.selected = sel);
    if let Some(w) = win(app) {
        update_rows(&w);
    }
}

fn clear_selection(app: &App) {
    if st(|s| !s.selected.is_empty()) {
        set_selected(app, Vec::new());
    }
}

// ---------- clicks and keys ----------

fn tap(app: &Rc<App>, id: &str, toggle: bool) {
    let at_level = level_ids().iter().any(|l| l == id);
    if at_level {
        if toggle {
            let mut sel = st(|s| s.selected.clone());
            match sel.iter().position(|s| s == id) {
                Some(i) => {
                    sel.remove(i);
                }
                None => sel.push(id.to_owned()),
            }
            set_selected(app, sel);
            return;
        }
        if st(|s| !s.selected.is_empty()) {
            // A plain click while a selection is active just clears it.
            clear_selection(app);
            return;
        }
    }
    activate(app, id);
}

fn activate(app: &Rc<App>, id: &str) {
    let island = find(&app.apps.borrow(), id).is_some_and(is_island);
    if island {
        open_island(app, Some(id.to_owned()));
    } else {
        launch_item(app, id);
    }
}

/// launchApp: off the UI thread, then the launch is recorded (feeds Recent).
fn launch_item(app: &Rc<App>, id: &str) {
    let entry = match items::launchable(&app.apps.borrow(), id) {
        Ok(e) => e.clone(),
        Err(e) => {
            modals::alert(app, &format!("Couldn't launch:\n{e}"));
            return;
        }
    };
    let (id, name) = (id.to_owned(), entry.str("name").unwrap_or("").to_owned());
    std::thread::spawn(move || {
        let _com = win::Com::init();
        let r = launch::launch_item(&entry.0).map_err(|e| e.to_string());
        post(move |app| match r {
            Ok(()) => {
                apply(app, |apps| items::record_launch(apps, &id, time::now_ms()));
            }
            Err(e) => modals::alert(app, &format!("Couldn't launch \"{name}\":\n{e}")),
        });
    });
}

fn edit(app: &Rc<App>, id: &str) {
    let rename = find(&app.apps.borrow(), id).is_some_and(|e| is_island(e) || e.str("type") == Some("stored"));
    if rename {
        modals::rename(app, id);
    } else {
        modals::edit_app(app, id);
    }
}

fn key(app: &Rc<App>, text: &str, ctrl: bool, alt: bool, _shift: bool) -> bool {
    use slint::platform::Key;
    let is = |k: Key| text == SharedString::from(k).as_str();
    let Some(w) = win(app) else { return false };
    let g = w.global::<GridState>();
    if is(Key::Escape) {
        w.global::<Popouts>().set_open("".into());
        clear_selection(app);
        return true;
    }
    let (dx, dy) = if is(Key::RightArrow) {
        (1, 0)
    } else if is(Key::LeftArrow) {
        (-1, 0)
    } else if is(Key::DownArrow) {
        (0, 1)
    } else if is(Key::UpArrow) {
        (0, -1)
    } else if is(Key::Return) {
        let id = st(|s| s.level.get(s.focus.max(0) as usize).filter(|_| s.focus >= 0).map(|l| l.0.clone()));
        if let Some(id) = id {
            activate(app, &id);
            return true;
        }
        return false;
    } else {
        // A letter or digit starts a search in the taskbar.
        let mut c = text.chars();
        if let (Some(ch), None, false, false) = (c.next(), c.next(), ctrl, alt) {
            if ch.is_ascii_alphanumeric() {
                super::taskbar::start_search(app, text);
                return true;
            }
        }
        return false;
    };
    let (focus, rect) = st(|s| {
        let tl: Vec<(f64, f64)> = s.level.iter().map(|l| (l.1 .1, l.1 .0)).collect();
        s.focus = logic::move_focus(&tl, s.focus, dx, dy);
        (s.focus, s.level.get(s.focus.max(0) as usize).map(|l| l.1))
    });
    g.set_focused(focus);
    if let Some(r) = rect {
        g.set_reveal_h(r.3 as f32);
        g.set_reveal_y(r.1 as f32);
    }
    true
}

// ---------- drag to move / reposition ----------

fn drag_start(app: &App, id: &str, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    st(|s| {
        let ids = if s.selected.len() > 1 && s.selected.iter().any(|x| x == id) { s.selected.clone() } else { vec![id.to_owned()] };
        s.drag = Some(Drag { ids, id: id.to_owned(), start: at });
    });
    let g = w.global::<GridState>();
    g.set_drag_dx(0.0);
    g.set_drag_dy(0.0);
    g.set_drop_crumb(-1);
    g.set_dragging(true);
    update_rows(&w);
}

/// A window point (logical) on the screen (physical), for the organizer panel.
fn screen_point(w: &MainWindow, at: (f64, f64)) -> Option<launchpad_core::organizer::Point> {
    let h = super::hwnd_of(w.window())?;
    let s = w.window().scale_factor() as f64;
    let mut p = windows::Win32::Foundation::POINT { x: (at.0 * s).round() as i32, y: (at.1 * s).round() as i32 };
    unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(h, &mut p) }.as_bool().then_some(launchpad_core::organizer::Point { x: p.x as f64, y: p.y as f64 })
}

/// The island at a canvas point (renderer.js addDropTarget on island tiles and cards).
fn island_at(p: (f64, f64)) -> Option<String> {
    st(|s| s.level.iter().rev().find(|l| l.2 && logic::contains(l.1, p.0, p.1)).map(|l| l.0.clone()))
}

fn drag_move(app: &App, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    let Some(start) = st(|s| s.drag.as_ref().map(|d| d.start)) else { return };
    let g = w.global::<GridState>();
    g.set_drag_dx((at.0 - start.0) as f32);
    g.set_drag_dy((at.1 - start.1) as f32);
    g.set_drag_wx(at.0 as f32);
    g.set_drag_wy(at.1 as f32);
    // Over the organizer panel's breadcrumb, when it carries the top bar.
    let screen = screen_point(&w, at);
    super::organizer::panel::set_drop_crumb(app, screen);
    let target = if in_grid(&w, at) { island_at(canvas_point(&w, at)) } else { None };
    g.set_drop_target(target.unwrap_or_default().into());
}

fn drag_end(app: &Rc<App>, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    let Some(drag) = st(|s| s.drag.take()) else { return };
    let g = w.global::<GridState>();
    g.set_dragging(false);
    g.set_drop_target("".into());
    let crumb = g.get_drop_crumb();
    g.set_drop_crumb(-1);
    update_rows(&w);
    let panel_crumb = screen_point(&w, at).and_then(super::organizer::panel::crumb_at);
    super::organizer::panel::set_drop_crumb(app, None);
    if let Some(target) = panel_crumb {
        return move_into(app, &drag.ids, &target);
    }

    if crumb >= 0 {
        let target = g.get_crumbs().row_data(crumb as usize).map(|c| c.id.to_string()).unwrap_or_default();
        return move_into(app, &drag.ids, &target);
    }
    if !in_grid(&w, at) {
        return;
    }
    let p = canvas_point(&w, at);
    if let Some(island) = island_at(p) {
        return move_into(app, &drag.ids, &island);
    }
    let (gx, gy) = logic::drop_cell(p.0, p.1);
    let folder = st(|s| s.folder.clone()).unwrap_or_default();
    let parent = find(&app.apps.borrow(), &drag.id).map(|e| e.str("parentId").unwrap_or("").to_owned());
    let Some(parent) = parent else { return };
    if parent != folder {
        // Out of an expanded island's card: a move to this level, at the drop cell.
        let id = drag.id.clone();
        apply(app, |apps| items::move_item(apps, &id, &folder, Some((gx, gy))));
        return;
    }
    if drag.ids.len() > 1 {
        change(app, |apps| {
            let d = find(apps, &drag.id);
            let dx = gx - d.and_then(|d| d.num("gridX")).unwrap_or(gx);
            let dy = gy - d.and_then(|d| d.num("gridY")).unwrap_or(gy);
            let moves: Vec<(String, f64, f64)> = drag
                .ids
                .iter()
                .map(|id| {
                    let it = find(apps, id);
                    let at = |k| it.and_then(|e| e.num(k)).unwrap_or(0.0);
                    (id.clone(), at("gridX") + dx, at("gridY") + dy)
                })
                .collect();
            let refs: Vec<(&str, f64, f64)> = moves.iter().map(|(i, x, y)| (i.as_str(), *x, *y)).collect();
            items::set_positions(apps, &refs);
            Ok(())
        });
    } else {
        apply(app, |apps| items::set_position(apps, &drag.id, gx, gy));
    }
}

/// moveItemsInto: into island `target` ("" = Home).
fn move_into(app: &Rc<App>, ids: &[String], target: &str) {
    let moving: Vec<&String> = ids.iter().filter(|id| !id.is_empty() && *id != target).collect();
    if moving.is_empty() {
        return;
    }
    change(app, |apps| {
        for id in moving {
            items::move_item(apps, id, target, None);
        }
        Ok(())
    });
}

// ---------- marquee ----------

fn marquee_start(app: &App, at: (f64, f64), additive: bool) {
    let Some(w) = win(app) else { return };
    let p = canvas_point(&w, at);
    let base = if additive { st(|s| s.selected.clone()) } else { Vec::new() };
    if !additive {
        clear_selection(app);
    }
    st(|s| s.marquee = Some(Marquee { start: p, base, additive, moved: false }));
}

fn marquee_move(app: &App, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    let cur = canvas_point(&w, at);
    let Some((start, mut next, moved)) = st(|s| {
        let m = s.marquee.as_mut()?;
        let r = (m.start.0.min(cur.0), m.start.1.min(cur.1), (cur.0 - m.start.0).abs(), (cur.1 - m.start.1).abs());
        m.moved |= r.2 > 3.0 || r.3 > 3.0;
        Some((r, m.base.clone(), m.moved))
    }) else {
        return;
    };
    for (id, rect, _) in st(|s| s.level.clone()) {
        if logic::overlaps(start, rect) && !next.contains(&id) {
            next.push(id);
        }
    }
    let g = w.global::<GridState>();
    g.set_marquee(moved);
    g.set_mq_x(start.0 as f32);
    g.set_mq_y(start.1 as f32);
    g.set_mq_w(start.2 as f32);
    g.set_mq_h(start.3 as f32);
    set_selected(app, next);
}

fn marquee_end(app: &Rc<App>) {
    let Some(m) = st(|s| s.marquee.take()) else { return };
    if let Some(w) = win(app) {
        w.global::<GridState>().set_marquee(false);
    }
    if !m.moved && !m.additive {
        clear_selection(app); // a plain click on empty space
    }
}

// ---------- island resize ----------

fn resize_start(app: &App, id: &str, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    let Some(e) = find(&app.apps.borrow(), id).cloned() else { return };
    let fp = layout::footprint_of(&e);
    st(|s| s.resize = Some(Resize { id: id.to_owned(), start: at, fp: (fp.w, fp.h), snap: (fp.w, fp.h) }));
    let g = w.global::<GridState>();
    g.set_guide_x((logic::cell_num(&e, "gridX") * CELL) as f32);
    g.set_guide_y((logic::cell_num(&e, "gridY") * CELL) as f32);
    g.set_rs_w((fp.w * CELL - CARD_MARGIN) as f32);
    g.set_rs_h((fp.h * CELL - CARD_MARGIN) as f32);
    g.set_snap_w(fp.w as i32);
    g.set_snap_h(fp.h as i32);
    g.set_resizing(id.into());
}

fn resize_move(app: &App, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    let Some((raw, snap)) = st(|s| {
        let r = s.resize.as_mut()?;
        // Follows the cursor; only the value committed on release snaps.
        let raw_w = (2.0 * CELL - CARD_MARGIN).max(r.fp.0 * CELL - CARD_MARGIN + at.0 - r.start.0);
        let raw_h = (2.0 * CELL - CARD_MARGIN).max(r.fp.1 * CELL - CARD_MARGIN + at.1 - r.start.1);
        let snap_of = |raw: f64, min, max| ((raw + CARD_MARGIN) / CELL).round().clamp(min, max);
        r.snap = (snap_of(raw_w, layout::MIN_ISLAND_W, layout::MAX_ISLAND_W), snap_of(raw_h, layout::MIN_ISLAND_H, layout::MAX_ISLAND_H));
        Some(((raw_w, raw_h), r.snap))
    }) else {
        return;
    };
    let g = w.global::<GridState>();
    g.set_rs_w(raw.0 as f32);
    g.set_rs_h(raw.1 as f32);
    g.set_snap_w(snap.0 as i32);
    g.set_snap_h(snap.1 as i32);
}

fn resize_end(app: &Rc<App>) {
    let Some(r) = st(|s| s.resize.take()) else { return };
    if let Some(w) = win(app) {
        w.global::<GridState>().set_resizing("".into());
    }
    if r.snap != r.fp {
        apply(app, |apps| items::set_size(apps, &r.id, r.snap.0, r.snap.1));
    }
}

// ---------- context menus ----------

fn entry(id: &str, title: impl Into<SharedString>) -> GridMenuEntry {
    GridMenuEntry { id: id.into(), title: title.into(), enabled: true, checked: false, danger: false }
}

fn set_menu(w: &MainWindow, a: Vec<GridMenuEntry>, sep: bool, mv: Vec<GridMenuEntry>, b: Vec<GridMenuEntry>, c: Vec<GridMenuEntry>) {
    let g = w.global::<GridState>();
    let m = |v: Vec<GridMenuEntry>| ModelRc::new(VecModel::from(v));
    g.set_menu_a(m(a));
    g.set_menu_sep(sep);
    g.set_menu_move(m(mv));
    g.set_menu_b(m(b));
    g.set_menu_c(m(c));
}

/// showItemContextMenu / showBulkContextMenu / showGridContextMenu, filled
/// just before the menu shows.
fn fill_menu(app: &App, id: &str, at: (f64, f64)) {
    let Some(w) = win(app) else { return };
    let at = canvas_point(&w, at);
    st(|s| s.menu = (id.to_owned(), at));
    if id.is_empty() {
        return set_menu(&w, vec![entry("new-island", "+ New Island"), entry("add-app", "+ Add App")], false, vec![], vec![], vec![]);
    }
    let sel = st(|s| s.selected.clone());
    if sel.len() > 1 && sel.iter().any(|s| s == id) {
        let n = sel.len();
        return set_menu(
            &w,
            vec![entry("bulk-pin", format!("Pin {n} to Taskbar")), entry("bulk-unpin", format!("Unpin {n} from Taskbar")), entry("bulk-move", "Move to Island…")],
            false,
            vec![],
            vec![],
            vec![entry("bulk-remove", format!("Remove {n} Items"))],
        );
    }
    let apps = app.apps.borrow();
    let Some(item) = find(&apps, id) else { return };
    let mut a = vec![entry("rename", "Rename")];
    if item.str("type") == Some("stored") {
        a.push(entry("original", "Show original location"));
        if item.str("originalPath").is_some_and(|o| !o.is_empty()) {
            a.push(entry("link", "Add as link"));
        }
        a.push(entry("thumb", "Refresh thumbnail"));
    } else if item.str("kind") == Some("app") {
        a.push(entry("edit", "Edit…"));
    }
    if item.str("special") == Some("recycleBin") {
        a.push(GridMenuEntry { danger: true, ..entry("empty-bin", "Empty Recycle Bin") });
    }
    let island = is_island(item);
    if island {
        let collapsed = item.get("collapsed") != Some(&serde_json::Value::Bool(false));
        a.push(entry("collapse", if collapsed { "Expand" } else { "Minimize" }));
        a.push(entry("names", if item.truthy("showNames") { "Hide App Names" } else { "Show App Names" }));
        let id = item.str("id").unwrap_or("");
        let has_scene = launchpad_core::organizer::layouts::load(&app.data_dir).scenes.iter().any(|s| s.island_id.as_deref() == Some(id));
        if has_scene {
            a.push(entry("scene-open", "Open as scene"));
        }
        a.push(entry("scene-save", "Save arrangement for this island"));
    }
    let mv = logic::move_targets(&apps, item)
        .into_iter()
        .map(|(label, parent, current)| GridMenuEntry { enabled: !current, checked: current, ..entry(&format!("move:{parent}"), label) })
        .collect();
    let b = vec![entry("move-dialog", "Move to Island…"), entry("pin", if item.truthy("pinned") { "Unpin from Taskbar" } else { "Pin to Taskbar" })];
    let c = vec![entry("remove", if island { "Remove Island" } else { "Remove" })];
    drop(apps);
    set_menu(&w, a, island, mv, b, c);
}

fn menu_pick(app: &Rc<App>, action: &str) {
    let (id, at) = st(|s| s.menu.clone());
    let folder = st(|s| s.folder.clone());
    if let Some(parent) = action.strip_prefix("move:") {
        let parent = parent.to_owned();
        apply(app, |apps| items::move_item(apps, &id, &parent, None));
        return;
    }
    let sel = st(|s| s.selected.clone());
    match action {
        "new-island" => modals::new_island(app, folder, Some(logic::drop_cell(at.0, at.1))),
        "add-app" => modals::add_app(app, folder, Some(logic::drop_cell(at.0, at.1))),
        "rename" => modals::rename(app, &id),
        "edit" => modals::edit_app(app, &id),
        "original" => show_original(app, &id),
        "link" => {
            let path = find(&app.apps.borrow(), &id).and_then(|e| e.str("originalPath").map(str::to_owned));
            if let Some(p) = path {
                quick_add(p);
            }
        }
        "thumb" => restore_thumb(app, id, true),
        "empty-bin" => modals::confirm(app, "Permanently empty the Recycle Bin? This cannot be undone.", "OK", false, |_| {
            std::thread::spawn(|| {
                let _com = win::Com::init();
                if let Err(e) = power::empty_recycle_bin() {
                    post(move |app| modals::alert(app, &format!("Couldn't empty the Recycle Bin:\n{e}")));
                }
            });
        }),
        "collapse" => {
            change(app, |apps| {
                let collapsed = find(apps, &id).is_some_and(|e| e.truthy("collapsed"));
                items::set_collapsed(apps, &id, !collapsed);
                Ok(())
            });
        }
        "names" => {
            change(app, |apps| {
                let on = find(apps, &id).is_some_and(|e| e.truthy("showNames"));
                items::set_show_names(apps, &id, !on);
                Ok(())
            });
        }
        "scene-open" => super::organizer::scenes::open_island(app, &id, |app, msg| modals::alert(app, &msg)),
        "scene-save" => {
            let msg = super::organizer::scenes::save_island(app, &id).unwrap_or_else(|e| e);
            modals::alert(app, &msg);
        }
        "move-dialog" => modals::move_items(app, vec![id]),
        "pin" => {
            apply(app, |apps| items::toggle_pin(apps, &id));
        }
        "remove" => remove_one(app, &id),
        "bulk-pin" | "bulk-unpin" => {
            let on = action == "bulk-pin";
            change(app, |apps| {
                for i in &sel {
                    items::set_pinned(apps, i, on);
                }
                Ok(())
            });
        }
        "bulk-move" => modals::move_items(app, sel),
        "bulk-remove" => bulk_remove(app, sel),
        _ => {}
    }
}

// ---------- item actions ----------

fn remove_one(app: &Rc<App>, id: &str) {
    let msg = {
        let apps = app.apps.borrow();
        let Some(item) = find(&apps, id) else { return };
        let name = item.str("name").unwrap_or("");
        if is_island(item) {
            format!("Remove island \"{name}\"? Its contents will move up one level.")
        } else if item.str("type") == Some("stored") {
            let at = item.str("originalPath").filter(|o| !o.is_empty()).map(|o| format!(" at {o}")).unwrap_or_default();
            format!("Remove \"{name}\"?\n\nLaunchpad's stored copy will be deleted permanently, not moved to the Recycle Bin. The original{at} is not touched.")
        } else {
            format!("Remove \"{name}\" from Launchpad? (This does not delete the app itself.)")
        }
    };
    let ids = vec![id.to_owned()];
    modals::confirm(app, &msg, "OK", false, move |app| remove_items(app, ids));
}

fn bulk_remove(app: &Rc<App>, ids: Vec<String>) {
    let stored = {
        let apps = app.apps.borrow();
        ids.iter().filter(|id| find(&apps, id).is_some_and(|e| e.str("type") == Some("stored"))).count()
    };
    let note = if stored > 0 {
        format!(
            "\n\n{} stored in Launchpad. {} will be deleted permanently, not moved to the Recycle Bin. The originals are not touched.",
            if stored == 1 { "1 of them is a file".to_owned() } else { format!("{stored} of them are files") },
            if stored == 1 { "Its copy" } else { "Those copies" }
        )
    } else {
        " (This does not delete the apps themselves.)".to_owned()
    };
    let msg = format!("Remove {} items from Launchpad?{note}", ids.len());
    modals::confirm(app, &msg, "OK", false, move |app| {
        clear_selection(app);
        remove_items(app, ids);
    });
}

/// apps:remove for each id; then, once saved, the icon file and any stored copy.
fn remove_items(app: &Rc<App>, ids: Vec<String>) {
    let Some(gone) = change(app, |apps| Ok(ids.iter().filter_map(|id| items::remove(apps, id).map(|r| (id.clone(), r.stored))).collect::<Vec<_>>()))
    else {
        return;
    };
    let (icons, dir) = (app.store.borrow().icons_dir(), app.data_dir.clone());
    std::thread::spawn(move || {
        for (id, stored) in gone {
            let _ = std::fs::remove_file(icons.join(format!("{id}.png")));
            if stored {
                if let Err(e) = file_store::remove_stored(&dir, &id) {
                    eprintln!("Failed to delete stored copy for {id}: {e}");
                }
            }
        }
    });
}

fn show_original(app: &Rc<App>, id: &str) {
    let path = find(&app.apps.borrow(), id).filter(|e| e.str("type") == Some("stored")).and_then(|e| e.str("originalPath").map(PathBuf::from));
    std::thread::spawn(move || {
        let _com = win::Com::init();
        let ok = path.filter(|p| !p.as_os_str().is_empty() && p.exists()).is_some_and(|p| launch::show_in_folder(&p).is_ok());
        if !ok {
            post(|app| modals::alert(app, "The original isn't there any more. The stored copy still works."));
        }
    });
}

/// files:restoreThumb: remakes a stored item's thumbnail off the UI thread.
fn restore_thumb(app: &App, id: String, report: bool) {
    let Some(e) = find(&app.apps.borrow(), &id).filter(|e| e.str("type") == Some("stored")).cloned() else { return };
    let dir = app.data_dir.clone();
    std::thread::spawn(move || {
        let target = PathBuf::from(e.str("target").unwrap_or(""));
        let state = if !target.as_os_str().is_empty() && target.exists() {
            let folder = e.str("storedKind") == Some("folder");
            Wic::new().map_err(|e| e.to_string()).and_then(|wic| {
                file_store::make_thumbnail(&dir, &target, &id, folder, &mut Thumbs(wic)).map_err(|e| e.to_string())
            })
        } else {
            Ok("generic") // nothing to make one from; stop asking
        };
        post(move |app| {
            let state = match state {
                Ok(s) => s,
                Err(err) if report => return modals::alert(app, &format!("Launchpad couldn't do that:\n{err}")),
                Err(_) => "generic",
            };
            app.images.forget(&file_store::thumbs_dir(&app.data_dir).join(format!("{id}.png")));
            change(app, |apps| {
                if let Some(e) = apps.iter_mut().find(|a| a.str("id") == Some(&id)) {
                    e.set("thumbState", state);
                }
                Ok(())
            });
        });
    });
}

/// quickAddFromPath: a link to `path` in the open island.
fn quick_add(path: String) {
    let parent = st(|s| s.folder.clone()).unwrap_or_default();
    std::thread::spawn(move || {
        let _com = win::Com::init();
        let lnk = items::is_shortcut(&path)
            .then(|| launch::read_shortcut(Path::new(&path)).ok())
            .flatten()
            .map(|s| Shortcut { target: s.target, args: s.args, cwd: s.cwd });
        post(move |app| {
            let form = logic::link_form(&path, &parent);
            let task = change(app, |apps| Ok(items::add_app(apps, &form, store::new_id(), time::now_ms(), |_| lnk)));
            if let Some(Some(task)) = task {
                super::icons::spawn(app, vec![task], |_| {});
            }
        });
    });
}

// ---------- files from Explorer ----------

fn ensure_drop_target(w: &MainWindow) {
    let Some(h) = super::hwnd_of(w.window()) else { return };
    if st(|s| std::mem::replace(&mut s.drop_hwnd, h.0 as isize)) == h.0 as isize {
        return;
    }
    if let Err(e) = drop::register(h, on_files, on_file_hover) {
        eprintln!("Couldn't accept files dropped from Explorer: {e}");
    }
}

fn on_file_hover(on: bool) {
    later(move |app| {
        if let Some(w) = win(app) {
            w.global::<GridState>().set_file_drag(on);
        }
    });
}

fn on_files(paths: Vec<String>, alt: bool, x: i32, y: i32) {
    later(move |app| {
        let Some(w) = win(app) else { return };
        let scale = w.window().scale_factor() as f64;
        let at = (x as f64 / scale, y as f64 / scale);
        // dropCell: outside the grid (the taskbar, the empty state) asks for 0,0.
        let cell = if in_grid(&w, at) && !w.global::<GridState>().get_empty() {
            let p = canvas_point(&w, at);
            logic::drop_cell(p.0, p.1)
        } else {
            (0.0, 0.0)
        };
        // Programs, scripts and shortcuts are always links; Alt or the setting links everything.
        let force = alt || file_store::file_store_options(&app.settings()).1 == "link";
        let (link, copy): (Vec<String>, Vec<String>) = paths.into_iter().partition(|p| force || file_store::is_linkable(p));
        for p in link {
            quick_add(p);
        }
        if !copy.is_empty() {
            store_dropped(app, copy, cell);
        }
    });
}

/// files:store, with the copying on a worker thread.
fn store_dropped(app: &Rc<App>, paths: Vec<String>, cell: (f64, f64)) {
    if app.store.borrow().is_read_only() {
        return modals::alert(app, &format!("Launchpad couldn't store those files:\n{READ_ONLY_MESSAGE}"));
    }
    let dir = app.data_dir.clone();
    let limit = file_store::file_store_options(&app.settings()).0;
    let parent = st(|s| s.folder.clone());
    std::thread::spawn(move || {
        let mut thumbs = match Wic::new() {
            Ok(w) => Thumbs(w),
            Err(e) => return post(move |app| modals::alert(app, &format!("Launchpad couldn't store those files:
{e}"))),
        };
        let mut done = Vec::new();
        for p in &paths {
            let r = file_store::store_from_path(&dir, Path::new(p), limit, file_store::DEFAULT_FILE_LIMIT, &mut thumbs);
            done.push(match r {
                Ok(s) => Ok(s),
                Err(err) => Err(StoreResult::Failed {
                    path: p.into(),
                    too_large: matches!(err, file_store::StoreError::TooLarge { .. }),
                    message: file_store::friendly_error(&err, Path::new(p)),
                }),
            });
        }
        post(move |app| stored(app, done, parent, cell));
    });
}

fn stored(app: &Rc<App>, done: Vec<std::result::Result<file_store::Stored, StoreResult>>, parent: Option<String>, cell: (f64, f64)) {
    let ok: Vec<&file_store::Stored> = done.iter().filter_map(|r| r.as_ref().ok()).collect();
    if !ok.is_empty() {
        let dir = app.data_dir.clone();
        let saved = app.mutate(|apps| {
            let parent = parent.filter(|p| find(apps, p).is_some_and(is_island));
            let opts = StoreOptions { parent_id: parent, grid_x: cell.0, grid_y: cell.1, ..StoreOptions::default() };
            for s in &ok {
                let e = file_store::new_entry(&dir, apps, s, &opts);
                apps.push(e);
            }
            Ok(())
        });
        if let Err(e) = saved {
            for s in &ok {
                let _ = file_store::remove_stored(&dir, &s.id);
            }
            return modals::alert(app, &format!("Launchpad couldn't store those files:\n{e}"));
        }
    }
    // One message for everything that failed; too-large ones can be links.
    let failed: Vec<(&PathBuf, bool, &String)> = done
        .iter()
        .filter_map(|r| match r {
            Err(StoreResult::Failed { path, too_large, message }) => Some((path, *too_large, message)),
            _ => None,
        })
        .collect();
    if failed.is_empty() {
        return;
    }
    let lines = failed.iter().map(|f| format!("• {}", f.2)).collect::<Vec<_>>().join("\n");
    let too_large: Vec<String> = failed.iter().filter(|f| f.1).map(|f| f.0.to_string_lossy().into_owned()).collect();
    if too_large.is_empty() {
        return modals::alert(app, &format!("Some items weren't stored:\n{lines}"));
    }
    let noun = if too_large.len() == 1 { "it".to_owned() } else { format!("those {}", too_large.len()) };
    modals::confirm(app, &format!("Some items weren't stored:\n{lines}\n\nAdd {noun} as a link to the original instead?"), "OK", false, move |_| {
        for p in too_large {
            quick_add(p);
        }
    });
}
