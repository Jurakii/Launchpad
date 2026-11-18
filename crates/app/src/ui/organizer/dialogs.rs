//! The organizer's dialogs in the main window (renderer.js in 1.2): the
//! layouts editor (Layouts in the top bar) and the Scenes dialog (Manage
//! scenes…, or Scenes… in the editor). The state lives here; the window may
//! be dropped and made again while one is open.
use super::scenes;
use crate::ui::{modals, App, LayoutCell, LayoutRow, MainWindow, OrganizerState, SceneRow};
use launchpad_core::ids::new_uuid;
use launchpad_core::organizer::cells::{self, Block};
use launchpad_core::organizer::layouts::{self as core_layouts, Library};
use launchpad_core::organizer::scenes::{self as core_scenes, SceneApp};
use launchpad_core::organizer::zones::Layout;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

const MAX_NAME: usize = 40;
const EDIT_HINT: &str = "Drag across cells to select zones. Merge makes them one zone; Split breaks them back into cells.";
const BUILT_IN_HINT: &str = "Built-in layouts can't be changed. Duplicate one to make your own version.";

#[derive(Default)]
struct State {
    layouts_open: bool,
    scenes_open: bool,
    editing: String,
    /// The user's own layout being edited (zone names as typed); None for a built-in.
    draft: Option<Layout>,
    selection: Option<Block>,
    anchor: Option<(i32, i32)>,
    focus_name: i32,
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    S.with_borrow_mut(f)
}

fn win(app: &App) -> Option<MainWindow> {
    app.window.borrow().as_ref().map(|w| w.clone_strong())
}

fn library(app: &App) -> Library {
    core_layouts::library(&core_layouts::load(&app.data_dir))
}

// ---------- layouts ----------

pub fn open_layouts(app: &Rc<App>) {
    let lib = library(app);
    let current = st(|s| s.editing.clone());
    let id = if lib.layouts.iter().any(|l| l.layout.id == current) {
        current
    } else {
        lib.layouts.iter().find(|l| !l.built_in).or(lib.layouts.first()).map(|l| l.layout.id.clone()).unwrap_or_default()
    };
    st(|s| s.layouts_open = true);
    select(app, &id);
    if let Some(w) = win(app) {
        w.global::<OrganizerState>().set_lay_gap(lib.gap.to_string().into());
    }
}

fn select(app: &App, id: &str) {
    let lib = library(app);
    let draft = lib.layouts.iter().find(|l| l.layout.id == id && !l.built_in).map(|l| cells::unname_zones(&l.layout));
    st(|s| {
        s.editing = id.to_owned();
        s.draft = draft;
        s.selection = None;
        s.anchor = None;
    });
    if let Some(w) = win(app) {
        let g = w.global::<OrganizerState>();
        let shown = st(|s| s.draft.clone()).or_else(|| lib.layouts.iter().find(|l| l.layout.id == id).map(|l| l.layout.clone()));
        g.set_lay_name(shown.map(|l| l.name).unwrap_or_default().into());
    }
    push(app);
}

fn cell_rows(layout: &Layout, selected: &[String]) -> ModelRc<LayoutCell> {
    ModelRc::new(VecModel::from(
        layout
            .zones
            .iter()
            .map(|z| LayoutCell { id: z.id.as_str().into(), name: z.name.as_str().into(), x: z.x, y: z.y, w: z.w, h: z.h, selected: selected.contains(&z.id) })
            .collect::<Vec<_>>(),
    ))
}

/// Everything but the two text fields being typed in (name, zone name).
fn push(app: &App) {
    let Some(w) = win(app) else { return };
    let g = w.global::<OrganizerState>();
    let (open, editing, draft, selection) = st(|s| (s.layouts_open, s.editing.clone(), s.draft.clone(), s.selection));
    g.set_lay_open(open);
    if !open {
        return;
    }
    let lib = library(app);
    let mut rows = Vec::new();
    let mut after_built_in = false;
    for l in &lib.layouts {
        // The one being edited shows the edits.
        let shown = if l.layout.id == editing { draft.clone().unwrap_or_else(|| l.layout.clone()) } else { l.layout.clone() };
        let tags: Vec<&str> = [l.built_in.then_some("Built-in"), l.hidden.then_some("Hidden")].into_iter().flatten().collect();
        rows.push(LayoutRow {
            id: l.layout.id.as_str().into(),
            name: shown.name.as_str().into(),
            tags: tags.join(" · ").into(),
            hidden: l.hidden,
            selected: l.layout.id == editing,
            sep_before: after_built_in && !l.built_in,
            cols: shown.cols,
            rows: shown.rows,
            zones: cell_rows(&shown, &[]),
        });
        after_built_in = l.built_in;
    }
    g.set_lay_list(ModelRc::new(VecModel::from(rows)));
    let Some(entry) = lib.layouts.iter().find(|l| l.layout.id == editing) else { return };
    let editable = draft.is_some();
    // Labels for unnamed zones on the canvas.
    let shown = draft.as_ref().map(cells::name_zones).unwrap_or_else(|| entry.layout.clone());
    let selected: Vec<String> =
        selection.map(|b| cells::zones_in(&shown.zones, b).into_iter().map(|z| z.id.clone()).collect()).unwrap_or_default();
    g.set_lay_editable(editable);
    g.set_lay_cols(shown.cols);
    g.set_lay_rows(shown.rows);
    g.set_lay_zones(cell_rows(&shown, &selected));
    g.set_lay_hint(if editable { EDIT_HINT } else { BUILT_IN_HINT }.into());
    g.set_lay_shown(!entry.hidden);
    let picked = draft.as_ref().zip(selection).map(|(d, b)| cells::zones_in(&d.zones, b)).unwrap_or_default();
    g.set_can_merge(picked.len() >= 2);
    g.set_can_split(picked.iter().any(|z| z.w * z.h > 1));
    g.set_zone_name_enabled(picked.len() == 1);
}

/// The zone name field, when the selection changes (not while typing in it).
fn push_zone_name(app: &App) {
    let Some(w) = win(app) else { return };
    let g = w.global::<OrganizerState>();
    let (draft, selection) = st(|s| (s.draft.clone(), s.selection));
    let single = draft.as_ref().zip(selection).map(|(d, b)| cells::zones_in(&d.zones, b)).filter(|z| z.len() == 1).map(|z| z[0].clone());
    match (single, draft) {
        (Some(z), Some(d)) => {
            g.set_zone_name(z.name.as_str().into());
            let named = cells::name_zones(&d);
            g.set_zone_placeholder(named.zones.iter().find(|n| n.id == z.id).map(|n| n.name.as_str()).unwrap_or("").into());
        }
        _ => {
            g.set_zone_name("".into());
            g.set_zone_placeholder("Select one zone".into());
        }
    }
}

/// Saves the draft (with every zone named); on failure says why and goes
/// back to what's saved.
fn change_draft(app: &Rc<App>, next: Layout, selection: Option<Block>) {
    st(|s| {
        s.draft = Some(next.clone());
        s.selection = selection;
    });
    if let Err(e) = core_layouts::save_layout(&app.data_dir, &cells::name_zones(&next)) {
        modals::alert(app, &format!("Launchpad couldn't save that:\n{e}"));
        let id = st(|s| s.editing.clone());
        select(app, &id);
        return;
    }
    super::layouts_changed(app);
    push(app);
}

fn select_cells(app: &App, a: (i32, i32), b: (i32, i32)) {
    let Some(draft) = st(|s| s.draft.clone()) else { return };
    let next = cells::expand_to_zones(&draft.zones, cells::cell_span(a, b));
    if st(|s| s.selection == Some(next)) {
        return;
    }
    st(|s| s.selection = Some(next));
    push(app);
    push_zone_name(app);
}

fn after_change(app: &Rc<App>, r: launchpad_core::Result<core_layouts::Data>) -> bool {
    match r {
        Ok(_) => {
            super::layouts_changed(app);
            true
        }
        Err(e) => {
            modals::alert(app, &format!("Launchpad couldn't save that:\n{e}"));
            false
        }
    }
}

fn new_layout(app: &Rc<App>) {
    let id = new_uuid();
    let layout = cells::name_zones(&Layout { id: id.clone(), ..cells::blank_layout("New layout", 4, 2) });
    if after_change(app, core_layouts::save_layout(&app.data_dir, &layout)) {
        select(app, &id);
        st(|s| s.focus_name += 1);
        if let Some(w) = win(app) {
            w.global::<OrganizerState>().set_lay_focus_name(st(|s| s.focus_name));
        }
    }
}

fn duplicate(app: &Rc<App>) {
    let (editing, draft) = st(|s| (s.editing.clone(), s.draft.clone()));
    let source = draft.map(|d| cells::name_zones(&d)).or_else(|| library(app).layouts.into_iter().find(|l| l.layout.id == editing).map(|l| l.layout));
    let Some(source) = source else { return };
    let id = new_uuid();
    let name: String = format!("{} copy", source.name).chars().take(MAX_NAME).collect();
    let copy = Layout { id: id.clone(), name, ..source };
    if after_change(app, core_layouts::save_layout(&app.data_dir, &copy)) {
        select(app, &id);
    }
}

fn delete(app: &Rc<App>) {
    let editing = st(|s| s.editing.clone());
    let lib = library(app);
    let Some(index) = lib.layouts.iter().position(|l| l.layout.id == editing && !l.built_in) else { return };
    let name = lib.layouts[index].layout.name.clone();
    modals::confirm(app, &format!("Delete the layout \"{name}\"? Windows already snapped with it stay where they are."), "OK", false, move |app| {
        if !after_change(app, core_layouts::remove_layout(&app.data_dir, &editing)) {
            return;
        }
        let lib = library(app);
        if let Some(l) = lib.layouts.get(index.min(lib.layouts.len().saturating_sub(1))) {
            select(app, &l.layout.id.clone());
        }
    });
}

fn set_gap(app: &Rc<App>, text: &str) {
    let lib = library(app);
    if let Ok(gap) = text.trim().parse::<i32>() {
        if gap != lib.gap {
            after_change(app, core_layouts::set_gap(&app.data_dir, gap));
        }
    }
    if let Some(w) = win(app) {
        w.global::<OrganizerState>().set_lay_gap(library(app).gap.to_string().into());
    }
}

fn close_layouts(app: &App) {
    st(|s| {
        s.layouts_open = false;
        s.anchor = None;
    });
    push(app);
}

// ---------- scenes ----------

pub fn open_scenes(app: &Rc<App>) {
    st(|s| s.scenes_open = true);
    push_scenes(app);
}

/// A scene was saved, renamed or removed: the open dialog follows.
pub fn scenes_changed(app: &App) {
    push_scenes(app);
}

fn local_time(utc_ms: f64) -> String {
    use windows::core::w;
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let t = ((utc_ms as i64 + 11_644_473_600_000).max(0) as u64) * 10_000;
    let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return String::new();
        }
        let (mut d, mut tm) = ([0u16; 64], [0u16; 64]);
        let dn = GetDateFormatEx(None, Default::default(), Some(&local), w!("MMM d"), Some(&mut d), None);
        let tn = GetTimeFormatEx(None, TIME_NOSECONDS, Some(&local), None, Some(&mut tm));
        format!("{}, {}", crate::win::from_wide(&d[..dn.max(0) as usize]), crate::win::from_wide(&tm[..tn.max(0) as usize]))
    }
}

/// "3 windows · Firefox, Code · Island: Work · saved Nov 18, 9:41 AM".
fn summary(app: &App, scene: &core_scenes::Scene) -> String {
    let apps = app.apps.borrow();
    let list: Vec<SceneApp> = apps
        .iter()
        .map(|e| SceneApp { kind: e.str("kind").unwrap_or(""), type_: e.str("type").unwrap_or(""), target: e.str("target").unwrap_or(""), name: e.str("name").unwrap_or("") })
        .collect();
    let mut names: Vec<String> = Vec::new();
    for w in &scene.windows {
        let n = core_scenes::app_name(&w.exe, &list);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let shown = if names.len() > 3 { format!("{} +{}", names[..3].join(", "), names.len() - 3) } else { names.join(", ") };
    let n = scene.windows.len();
    let mut parts = vec![format!("{n} window{}", if n == 1 { "" } else { "s" }), shown];
    if let Some(island) = scene.island_id.as_ref().and_then(|i| apps.iter().find(|e| e.str("id") == Some(i) && e.str("kind") == Some("island"))) {
        parts.push(format!("Island: {}", island.str("name").unwrap_or("")));
    }
    if let Some(at) = scene.saved_at {
        parts.push(format!("saved {}", local_time(at)));
    }
    parts.join(" · ")
}

fn push_scenes(app: &App) {
    let Some(w) = win(app) else { return };
    let g = w.global::<OrganizerState>();
    let open = st(|s| s.scenes_open);
    g.set_sc_open(open);
    if !open {
        return;
    }
    let rows: Vec<SceneRow> = core_layouts::load(&app.data_dir)
        .scenes
        .iter()
        .map(|s| SceneRow { id: s.id.as_str().into(), name: s.name.as_str().into(), meta: summary(app, s).into() })
        .collect();
    g.set_sc_rows(ModelRc::new(VecModel::from(rows)));
}

fn scene_result(app: &Rc<App>, r: Result<(), String>) {
    if let Err(e) = r {
        modals::alert(app, &e);
    }
    push_scenes(app);
}

/// Re-pushed with the rest of the window (not the fields being typed in).
pub fn refresh(app: &App) {
    push(app);
    push_scenes(app);
}

/// A new window (the old one was dropped on hide) gets the typed fields too.
fn fill_texts(app: &App) {
    let Some(w) = win(app) else { return };
    let g = w.global::<OrganizerState>();
    let (editing, draft) = st(|s| (s.editing.clone(), s.draft.clone()));
    let name = draft.map(|d| d.name).or_else(|| library(app).layouts.into_iter().find(|l| l.layout.id == editing).map(|l| l.layout.name));
    g.set_lay_name(name.unwrap_or_default().into());
    g.set_lay_gap(library(app).gap.to_string().into());
    push_zone_name(app);
}

// Callbacks run inside the window's event handling; anything that may open
// a dialog or rebuild models waits for `later`.
pub fn bind(_app: &Rc<App>, w: &MainWindow) {
    super::later(|app| fill_texts(app));
    let g = w.global::<OrganizerState>();
    g.on_lay_select(move |id| {
        let id = id.to_string();
        super::later(move |app| select(app, &id));
    });
    g.on_lay_cell_down(move |x, y| {
        super::later(move |app| {
            st(|s| s.anchor = Some((x, y)));
            select_cells(app, (x, y), (x, y));
        })
    });
    g.on_lay_cell_move(move |x, y| {
        super::later(move |app| {
            if let Some(a) = st(|s| s.anchor) {
                select_cells(app, a, (x, y));
            }
        })
    });
    g.on_lay_merge(move || {
        super::later(|app| {
            let Some((d, b)) = st(|s| s.draft.clone().zip(s.selection)) else { return };
            let next = cells::merge_zones(&d, b);
            let sel = cells::expand_to_zones(&next.zones, b);
            change_draft(app, next, Some(sel));
            push_zone_name(app);
        })
    });
    g.on_lay_split(move || {
        super::later(|app| {
            let Some((d, b)) = st(|s| s.draft.clone().zip(s.selection)) else { return };
            change_draft(app, cells::split_zones(&d, b), Some(b));
            push_zone_name(app);
        })
    });
    g.on_lay_zone_name(move |text| {
        let name = text.trim().to_owned();
        super::later(move |app| {
            let Some((mut d, b)) = st(|s| s.draft.clone().zip(s.selection)) else { return };
            let picked: Vec<String> = cells::zones_in(&d.zones, b).into_iter().map(|z| z.id.clone()).collect();
            let [id] = picked.as_slice() else { return };
            if let Some(z) = d.zones.iter_mut().find(|z| z.id == *id) {
                z.name = name.chars().take(MAX_NAME).collect();
            }
            change_draft(app, d, Some(b));
        })
    });
    g.on_lay_name_changed(move |text| {
        let name = text.trim().to_owned();
        super::later(move |app| {
            // A layout needs a name: an empty field isn't saved.
            let Some(d) = st(|s| s.draft.clone()).filter(|d| !name.is_empty() && d.name != name) else { return };
            let sel = st(|s| s.selection);
            change_draft(app, Layout { name: name.chars().take(MAX_NAME).collect(), ..d }, sel);
        })
    });
    g.on_lay_size(move |c, r| {
        super::later(move |app| {
            let Some(d) = st(|s| s.draft.clone()).filter(|d| d.cols != c || d.rows != r) else { return };
            change_draft(app, cells::resize_grid(&d, c, r), None);
            push_zone_name(app);
        })
    });
    g.on_lay_shown_toggled(move |on| {
        super::later(move |app| {
            let id = st(|s| s.editing.clone());
            after_change(app, core_layouts::set_hidden(&app.data_dir, &id, !on));
            push(app);
        })
    });
    g.on_lay_new(move || super::later(new_layout));
    g.on_lay_duplicate(move || super::later(duplicate));
    g.on_lay_delete(move || super::later(delete));
    g.on_lay_gap_changed(move |t| {
        let t = t.to_string();
        super::later(move |app| set_gap(app, &t));
    });
    g.on_lay_scenes(move || {
        super::later(|app| {
            close_layouts(app);
            open_scenes(app);
        })
    });
    g.on_lay_close(move || super::later(|app| close_layouts(app)));

    g.on_sc_rename(move |id, name| {
        let (id, name) = (id.to_string(), name.to_string());
        super::later(move |app| scene_result(app, core_layouts::rename_scene(&app.data_dir, &id, &name).map(|_| ()).map_err(|e| e.to_string())));
    });
    g.on_sc_apply(move |id| {
        let id = id.to_string();
        super::later(move |app| {
            let msg = scenes::apply(app, &id).unwrap_or_else(|e| e);
            modals::alert(app, &msg);
        });
    });
    g.on_sc_save_again(move |id| {
        let id = id.to_string();
        super::later(move |app| scene_result(app, scenes::save_over(app, &id).map(|_| ())));
    });
    g.on_sc_delete(move |id| {
        let id = id.to_string();
        super::later(move |app| {
            let Some(name) = core_layouts::load(&app.data_dir).scenes.into_iter().find(|s| s.id == id).map(|s| s.name) else { return };
            modals::confirm(app, &format!("Delete the scene \"{name}\"? Your windows stay where they are."), "OK", false, move |app| {
                scene_result(app, core_layouts::remove_scene(&app.data_dir, &id).map(|_| ()).map_err(|e| e.to_string()));
            });
        });
    });
    g.on_sc_save(move || super::later(|app| scene_result(app, scenes::save(app, None, None, None).map(|_| ()))));
    g.on_sc_close(move || {
        super::later(|app| {
            st(|s| s.scenes_open = false);
            push_scenes(app);
        })
    });
}
