//! See ui/modals.slint. The functions below are what the other parts call;
//! their signatures are fixed (other parts are written against them).
//!
//! All modal state lives here (the window is dropped on hide), in `ST`, and
//! `push` copies it into `ModalState`. Ported from renderer.js: "Custom,
//! non-blocking replacements for window.alert()/confirm()", "add / edit app
//! modal", "new island / rename modal", "move-to-island modal", and the
//! load-issue notice.
use super::{icons, images, App, MainWindow, ModalState, MoveRow};
use crate::win::dialogs;
use crate::win::image::{large_icon_size, Rgba, Wic};
use crate::win::launch;
use crate::win::services::Icons;
use launchpad_core::items::{self, AppForm, IconTask, Shortcut};
use launchpad_core::store::{self, Entry, LoadIssue, LoadIssueState};
use launchpad_core::{favicon, file_store, time};
use slint::{Color, ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// renderer.js ISLAND_COLORS.
pub const ISLAND_COLORS: [&str; 8] = ["#5b8cff", "#ff8a5b", "#5bd6ff", "#ffd75b", "#ff5b8a", "#8a5bff", "#5bffb0", "#9aa0ac"];
/// The Type select's options, in order.
const TYPES: [&str; 4] = ["path", "url", "script", "command"];
/// The preview is drawn at 46 px; decoded at twice that for high-DPI screens.
const PREVIEW_PX: u32 = 92;

type OnOk = Box<dyn FnOnce(&Rc<App>)>;

struct Dialog {
    message: String,
    cancel: bool,
    ok_label: String,
    danger: bool,
    on_ok: Option<OnOk>,
}

#[derive(Default)]
struct Form {
    editing: Option<String>,
    parent_id: Option<String>,
    cell: Option<(f64, f64)>,
    stored: bool,
    stored_target: String,
    name: String,
    type_: usize,
    target: String,
    url: String,
    interpreter: String,
    args: String,
    cwd_script: String,
    command: String,
    cwd_command: String,
    icon: Option<Image>,
    /// pendingCustomIconPath / pendingAutoIcon.
    custom: Option<String>,
    auto: bool,
    /// Which opening of the form a preview on its way belongs to.
    gen: u64,
}

struct NameForm {
    /// None: a new island. Some((id, is_island)): renaming.
    target: Option<(String, bool)>,
    parent_id: Option<String>,
    cell: Option<(f64, f64)>,
    text: String,
    colors: bool,
    color: String,
    hex_open: bool,
    hex: String,
}

struct MoveForm {
    ids: Vec<String>,
    /// (label, parent id, is the current parent)
    rows: Vec<(String, Option<String>, bool)>,
}

#[derive(Default)]
struct State {
    dialogs: VecDeque<Dialog>,
    form: Option<Form>,
    name: Option<NameForm>,
    moving: Option<MoveForm>,
    focus_tick: i32,
    gen: u64,
    started: bool,
}

thread_local! {
    static ST: RefCell<State> = RefCell::default();
}

// Never call another function that uses `st` (or runs callbacks) inside `f`.
fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    ST.with(|s| f(&mut s.borrow_mut()))
}

fn s(v: &str) -> SharedString {
    v.into()
}

/// JS `String.prototype.trim` (it also strips U+FEFF).
pub(crate) fn js_trim(v: &str) -> String {
    v.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').to_string()
}

fn push(w: &MainWindow) {
    let g = w.global::<ModalState>();
    st(|st| {
        match st.dialogs.front() {
            Some(d) => {
                g.set_dialog_message(s(&d.message));
                g.set_dialog_cancel(d.cancel);
                g.set_dialog_ok_label(s(&d.ok_label));
                g.set_dialog_danger(d.danger);
                g.set_dialog_open(true);
            }
            None => g.set_dialog_open(false),
        }
        match &st.form {
            Some(f) => {
                g.set_app_title(s(if f.editing.is_some() { "Edit App" } else { "Add App" }));
                g.set_app_editing(f.editing.is_some());
                g.set_app_stored(f.stored);
                g.set_app_has_icon(f.icon.is_some());
                g.set_app_icon(f.icon.clone().unwrap_or_default());
                g.set_f_name(s(&f.name));
                g.set_f_type(f.type_ as i32);
                g.set_f_target(s(&f.target));
                g.set_f_url(s(&f.url));
                g.set_f_interpreter(s(&f.interpreter));
                g.set_f_args(s(&f.args));
                g.set_f_cwd_script(s(&f.cwd_script));
                g.set_f_command(s(&f.command));
                g.set_f_cwd_command(s(&f.cwd_command));
                g.set_app_open(true);
            }
            None => g.set_app_open(false),
        }
        match &st.name {
            Some(n) => {
                g.set_name_title(s(if n.target.is_some() { "Rename" } else { "New Island" }));
                g.set_name_text(s(&n.text));
                g.set_name_colors(n.colors);
                let sel = ISLAND_COLORS.iter().position(|c| *c == n.color);
                g.set_name_selected(sel.map_or(-1, |i| i as i32));
                g.set_name_custom(super::prefs::parse_hex(&n.color).unwrap_or_default());
                g.set_name_hex_open(n.hex_open);
                g.set_name_hex(s(&n.hex));
                g.set_name_open(true);
            }
            None => g.set_name_open(false),
        }
        match &st.moving {
            Some(m) => {
                let rows: Vec<MoveRow> = m.rows.iter().map(|(label, _, current)| MoveRow { label: s(label), current: *current }).collect();
                g.set_move_rows(ModelRc::new(VecModel::from(rows)));
                g.set_move_open(true);
            }
            None => g.set_move_open(false),
        }
        g.set_focus_tick(st.focus_tick);
    });
}

fn redraw(app: &App) {
    if let Some(w) = app.window.borrow().as_ref() {
        push(w);
    }
}

/// After something closed: the keyboard focus goes to whatever is on top now.
/// Waits a turn of the event loop, so the closed modal's elements are gone
/// before the focus moves (focusing while they're torn down loses it).
fn refocus(app: &App) {
    redraw(app);
    super::later(|app| {
        let any = st(|st| {
            st.focus_tick += 1;
            !st.dialogs.is_empty() || st.form.is_some() || st.name.is_some() || st.moving.is_some()
        });
        redraw(app);
        if !any {
            super::settings::refocus(app);
        }
    });
}

/// Wires the global's callbacks on a newly made window.
pub fn bind(app: &Rc<App>, w: &MainWindow) {
    let g = w.global::<ModalState>();
    let swatches: Vec<Color> = ISLAND_COLORS.iter().filter_map(|c| super::prefs::parse_hex(c)).collect();
    g.set_swatches(ModelRc::new(VecModel::from(swatches)));

    g.on_dialog_done(|ok| {
        super::with(|app| dialog_done(app, ok));
    });

    g.on_app_field(|key, value| {
        st(|st| {
            let Some(f) = st.form.as_mut() else { return };
            let v = value.to_string();
            match key.as_str() {
                "name" => f.name = v,
                "type" => f.type_ = v.parse::<usize>().unwrap_or(0).min(3),
                "target" => f.target = v,
                "url" => f.url = v,
                "interpreter" => f.interpreter = v,
                "args" => f.args = v,
                "cwd-script" => f.cwd_script = v,
                "command" => f.command = v,
                "cwd-command" => f.cwd_command = v,
                _ => {}
            }
        });
    });
    // Dialogs run their own message loop: start them from a timer, not from
    // inside Slint's event dispatch.
    g.on_browse(|folder| super::later(move |app| browse(app, folder)));
    g.on_choose_icon(|| super::later(choose_icon));
    g.on_auto_icon(|| {
        super::with(auto_icon);
    });
    g.on_app_remove(|| {
        super::with(remove_from_form);
    });
    g.on_app_save(|| {
        super::with(save_form);
    });
    g.on_app_cancel(|| {
        super::with(|app| {
            st(|st| st.form = None);
            refocus(app);
        });
    });

    g.on_name_field(|t| st(|st| st.name.iter_mut().for_each(|n| n.text = t.to_string())));
    g.on_name_pick(|i| {
        super::with(|app| {
            st(|st| {
                if let (Some(n), Some(c)) = (st.name.as_mut(), ISLAND_COLORS.get(i as usize)) {
                    n.color = c.to_string();
                    n.hex = n.color.clone();
                }
            });
            redraw(app);
        });
    });
    g.on_name_custom_clicked(|| {
        super::with(|app| {
            st(|st| {
                if let Some(n) = st.name.as_mut() {
                    n.hex_open = true;
                    n.hex = n.color.clone();
                }
            });
            redraw(app);
        });
    });
    g.on_name_hex_edited(|t| {
        super::with(|app| {
            let hex = normalize_hex(&t);
            st(|st| {
                if let Some(n) = st.name.as_mut() {
                    n.hex = t.to_string();
                    if let Some(h) = hex.clone() {
                        n.color = h;
                    }
                }
            });
            if hex.is_some() {
                redraw(app);
            }
        });
    });
    g.on_name_save(|| {
        super::with(save_name);
    });
    g.on_name_cancel(|| {
        super::with(|app| {
            st(|st| st.name = None);
            refocus(app);
        });
    });

    g.on_move_pick(|i| {
        super::with(|app| move_pick(app, i as usize));
    });
    g.on_move_cancel(|| {
        super::with(|app| {
            st(|st| st.moving = None);
            refocus(app);
        });
    });

    push(w);

    // Once per run: the load-issue notice (renderer.js showLoadIssue), and
    // in a debug build the test hook.
    if !st(|st| std::mem::replace(&mut st.started, true)) {
        let issue = app.store.borrow().get_load_issue();
        if let Some(issue) = issue {
            alert(app, &load_issue_message(&issue));
        }
        #[cfg(debug_assertions)]
        if let Ok(what) = std::env::var("LAUNCHPAD_DEBUG_OPEN") {
            super::later(move |app| debug_open(app, &what));
        }
    }
}

/// Pushes the current state into the window.
pub fn refresh(_app: &App, w: &MainWindow) {
    push(w);
}

// ---------- alert / confirm ----------

fn queue(app: &Rc<App>, d: Dialog) {
    let first = st(|st| {
        st.dialogs.push_back(d);
        st.dialogs.len() == 1
    });
    if first {
        st(|st| st.focus_tick += 1);
    }
    redraw(app);
}

fn dialog_done(app: &Rc<App>, ok: bool) {
    let Some(d) = st(|st| st.dialogs.pop_front()) else { return };
    refocus(app);
    if ok {
        if let Some(f) = d.on_ok {
            f(app);
        }
    }
}

/// A message with an OK button (1.2's non-blocking showAlert).
pub fn alert(app: &Rc<App>, message: &str) {
    queue(app, Dialog { message: message.to_string(), cancel: false, ok_label: "OK".into(), danger: false, on_ok: None });
}

/// A question with Cancel and `ok_label`; `on_ok` runs only if confirmed.
/// `danger` styles the button red (1.2's showConfirm).
pub fn confirm(app: &Rc<App>, message: &str, ok_label: &str, danger: bool, on_ok: impl FnOnce(&Rc<App>) + 'static) {
    queue(app, Dialog { message: message.to_string(), cancel: true, ok_label: ok_label.to_string(), danger, on_ok: Some(Box::new(on_ok)) });
}

/// renderer.js `refreshFromServer`'s message for a failed change.
pub(crate) fn alert_failed(app: &Rc<App>, e: &launchpad_core::Error) {
    alert(app, &format!("Launchpad couldn't do that:\n{e}"));
}

fn load_issue_message(issue: &LoadIssue) -> String {
    match issue.state {
        LoadIssueState::Restored => format!(
            "Your saved apps file couldn't be read, so Launchpad restored the previous version from its backup. \
             Your most recent change may be missing.\n\nThe unreadable file was kept at:\n{}",
            issue.corrupt_copy.display()
        ),
        _ => format!(
            "Your saved apps file couldn't be read and there's no usable backup, so Launchpad opened empty \
             and won't save changes - that way nothing gets overwritten.\n\nA copy of the unreadable file is at:\n{}\n\n\
             Fix or replace:\n{}\nor import a backup from Settings.",
            issue.corrupt_copy.display(),
            issue.apps_file.display()
        ),
    }
}

// ---------- add / edit app ----------

fn find(app: &App, id: &str) -> Option<Entry> {
    app.apps.borrow().iter().find(|a| a.str("id") == Some(id)).cloned()
}

fn open_form(app: &Rc<App>, mut f: Form) {
    st(|st| {
        st.gen += 1;
        f.gen = st.gen;
        st.form = Some(f);
        st.focus_tick += 1;
    });
    redraw(app);
}

/// The Add App dialog, adding into island `parent_id` (None = Home), at
/// `cell` if given (else the first free cell).
pub fn add_app(app: &Rc<App>, parent_id: Option<String>, cell: Option<(f64, f64)>) {
    open_form(app, Form { parent_id, cell, ..Default::default() });
}

/// The Edit dialog for app `id`.
pub fn edit_app(app: &Rc<App>, id: &str) {
    let Some(e) = find(app, id) else { return };
    let text = |k: &str| e.str(k).unwrap_or("").to_string();
    let type_ = text("type");
    let icon = e.str("iconPath").filter(|p| !p.is_empty()).and_then(|p| app.images.icon(Path::new(p), PREVIEW_PX));
    open_form(
        app,
        Form {
            editing: Some(id.to_string()),
            stored: type_ == "stored",
            stored_target: text("target"),
            name: text("name"),
            type_: TYPES.iter().position(|t| *t == type_).unwrap_or(0),
            target: text("target"),
            url: if type_ == "url" { text("target") } else { String::new() },
            interpreter: text("interpreter"),
            args: text("args"),
            cwd_script: text("cwd"),
            command: text("command"),
            cwd_command: text("cwd"),
            icon,
            ..Default::default()
        },
    );
}

/// The form as renderer.js's submit handler builds `formData`.
fn form_data(f: &Form) -> AppForm {
    let t = TYPES[f.type_];
    if f.stored {
        return AppForm { name: js_trim(&f.name), type_: "stored".into(), target: f.stored_target.clone(), ..Default::default() };
    }
    AppForm {
        name: js_trim(&f.name),
        type_: t.into(),
        target: js_trim(if t == "url" { &f.url } else { &f.target }),
        args: if t == "script" { js_trim(&f.args) } else { String::new() },
        cwd: match t {
            "script" => js_trim(&f.cwd_script),
            "command" => js_trim(&f.cwd_command),
            _ => String::new(),
        },
        interpreter: if t == "script" {
            Some(js_trim(&f.interpreter)).filter(|i| !i.is_empty()).unwrap_or_else(|| "python".into())
        } else {
            String::new()
        },
        command: if t == "command" { js_trim(&f.command) } else { String::new() },
        parent_id: f.parent_id.clone().unwrap_or_default(),
        custom_icon_path: f.custom.clone().unwrap_or_default(),
        force_auto_icon: f.auto,
    }
}

fn save_form(app: &Rc<App>) {
    let Some((data, editing, cell)) = st(|st| st.form.as_ref().map(|f| (form_data(f), f.editing.clone(), f.cell))) else { return };
    if let Err(e) = items::validate_form(&data) {
        alert(app, &e.to_string());
        return;
    }
    st(|st| st.form = None);
    refocus(app);
    // A .lnk is read off the UI thread; everything else is saved straight away.
    if data.type_ == "path" && items::is_shortcut(&data.target) {
        let target = data.target.clone();
        std::thread::spawn(move || {
            let lnk = launch::read_shortcut(Path::new(&target)).ok().map(|s| Shortcut { target: s.target, args: s.args, cwd: s.cwd });
            super::post(move |app| finish_save(app, data, editing, cell, lnk));
        });
    } else {
        finish_save(app, data, editing, cell, None);
    }
}

fn finish_save(app: &Rc<App>, data: AppForm, editing: Option<String>, cell: Option<(f64, f64)>, lnk: Option<Shortcut>) {
    let icons_dir = app.store.borrow().icons_dir();
    let res = app.mutate(|apps| {
        let task = match &editing {
            Some(id) => items::update_app(apps, id, &data, move |_| lnk),
            None => {
                let id = store::new_id();
                let task = items::add_app(apps, &data, id.clone(), time::now_ms(), move |_| lnk);
                if let Some((x, y)) = cell {
                    items::set_position(apps, &id, x, y);
                }
                task
            }
        };
        match task {
            // Saved before the list: if the image won't read, nothing is kept (as 1.2.2).
            Some(t @ IconTask::SaveCustom { .. }) => {
                let path = icons::run(&t, &icons_dir, false)?;
                if let Some(p) = &path {
                    app.images.forget(Path::new(p));
                }
                items::apply_icon(apps, &t, path);
                Ok(None)
            }
            other => Ok(other),
        }
    });
    match res {
        Ok(Some(task)) => icons::spawn(app, vec![task], |_| {}),
        Ok(None) => {}
        Err(e) => alert_failed(app, &e),
    }
}

enum Preview {
    Extract(PathBuf),
    Favicon(String, bool),
}

/// Fetches an icon preview on a worker thread; `done` gets it on the UI
/// thread, only if the same opening of the form is still up.
fn preview(gen: u64, job: Preview, done: impl FnOnce(&Rc<App>, Option<Rgba>) + Send + 'static) {
    std::thread::spawn(move || {
        let img = match job {
            Preview::Extract(p) => Wic::new().and_then(|w| w.file_icon(&p, large_icon_size())).ok(),
            Preview::Favicon(url, google) => Icons::new().ok().and_then(|mut src| favicon::fetch_favicon(&url, google, &mut src)),
        };
        super::post(move |app| {
            if st(|st| st.form.as_ref().map(|f| f.gen)) == Some(gen) {
                done(app, img);
            }
        });
    });
}

/// `dialog:pickTarget` and `pickAndFillTarget`.
fn browse(app: &Rc<App>, folder: bool) {
    if st(|st| st.form.is_none()) {
        return;
    }
    let owner = app.hwnd();
    let picked = if folder {
        dialogs::open_folder(owner, None)
    } else {
        let filters: [dialogs::Filter; 3] =
            [("Programs & Shortcuts", &["exe", "lnk", "bat", "cmd"]), ("Scripts", &["py", "js", "mjs", "ps1"]), ("All Files", &["*"])];
        dialogs::open_file(owner, &filters, None)
    };
    let path = match picked {
        Ok(Some(p)) => p,
        Ok(None) => return,
        Err(e) => return alert(app, &e.to_string()),
    };
    fill_target(app, path);
}

/// What `dialog:pickTarget` did with the picked file or folder.
pub(crate) fn fill_target(app: &Rc<App>, path: PathBuf) {
    let target = path.to_string_lossy().into_owned();
    let (type_, interpreter) = items::infer_type_from_ext(&target);
    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(gen) = st(|st| {
        let f = st.form.as_mut()?;
        f.target = target.clone();
        f.type_ = TYPES.iter().position(|t| *t == type_).unwrap_or(0);
        if !interpreter.is_empty() {
            f.interpreter = interpreter.into();
        }
        if f.name.is_empty() {
            f.name = name;
        }
        Some(f.gen)
    }) else {
        return;
    };
    redraw(app);
    preview(gen, Preview::Extract(path), |app, img| {
        let Some(img) = img else { return };
        st(|st| {
            if let Some(f) = st.form.as_mut().filter(|f| f.custom.is_none()) {
                // A new target was picked, so its icon wins on save even over a chosen one.
                f.icon = Some(images::to_image(img));
                f.auto = true;
            }
        });
        redraw(app);
    });
}

fn auto_icon(app: &Rc<App>) {
    let Some((gen, is_url, value)) =
        st(|st| st.form.as_ref().map(|f| (f.gen, TYPES[f.type_] == "url", js_trim(if TYPES[f.type_] == "url" { &f.url } else { &f.target }))))
    else {
        return;
    };
    let job = if is_url {
        if value.is_empty() {
            return alert(app, "Enter a URL first, then auto-detect its icon.");
        }
        Preview::Favicon(value, icons::google_fallback(app))
    } else {
        if value.is_empty() {
            return alert(app, "Set a target file first, then auto-detect its icon.");
        }
        Preview::Extract(PathBuf::from(value))
    };
    preview(gen, job, move |app, img| {
        let Some(img) = img else {
            if is_url {
                alert(app, "Couldn't find an icon for that website.");
            }
            return; // detection failed: leave whatever icon is already set alone
        };
        st(|st| {
            if let Some(f) = st.form.as_mut() {
                f.custom = None;
                f.auto = true;
                f.icon = Some(images::to_image(img));
            }
        });
        redraw(app);
    });
}

/// `dialog:pickIcon`.
fn choose_icon(app: &Rc<App>) {
    if st(|st| st.form.is_none()) {
        return;
    }
    let filters: [dialogs::Filter; 1] = [("Images", &["png", "ico", "jpg", "jpeg", "bmp"])];
    let path = match dialogs::open_file(app.hwnd(), &filters, None) {
        Ok(Some(p)) => p,
        Ok(None) => return,
        Err(e) => return alert(app, &e.to_string()),
    };
    set_custom_icon(app, path);
}

fn set_custom_icon(app: &Rc<App>, path: PathBuf) {
    let img = app.images.icon(&path, PREVIEW_PX);
    st(|st| {
        if let Some(f) = st.form.as_mut() {
            f.custom = Some(path.to_string_lossy().into_owned());
            f.auto = false;
            if img.is_some() {
                f.icon = img;
            }
        }
    });
    redraw(app);
}

fn remove_from_form(app: &Rc<App>) {
    let Some(id) = st(|st| st.form.as_ref().and_then(|f| f.editing.clone())) else { return };
    confirm(app, "Remove this app from the launcher? (This does not delete the app itself.)", "OK", false, move |app| {
        st(|st| st.form = None);
        refocus(app);
        remove_item(app, &id);
    });
}

/// `apps:remove`, then its icon and (for a stored item) its copy.
pub(crate) fn remove_item(app: &Rc<App>, id: &str) {
    match app.mutate(|apps| Ok(items::remove(apps, id))) {
        Ok(Some(removed)) => {
            let icon = app.store.borrow().icons_dir().join(format!("{id}.png"));
            if icon.exists() {
                if let Err(e) = std::fs::remove_file(&icon) {
                    eprintln!("Removing {}: {e}", icon.display());
                }
                app.images.forget(&icon);
            }
            if removed.stored {
                let (dir, id) = (app.data_dir.clone(), id.to_string());
                std::thread::spawn(move || {
                    if let Err(e) = file_store::remove_stored(&dir, &id) {
                        eprintln!("Failed to delete the stored copy of {id}: {e}");
                    }
                });
            }
        }
        Ok(None) => {}
        Err(e) => alert_failed(app, &e),
    }
}

// ---------- new island / rename ----------

/// "#rgb" or "#rrggbb" as `<input type=color>` gives it: "#rrggbb", lower case.
/// The leading "#" may be left out when typing.
pub(crate) fn normalize_hex(v: &str) -> Option<String> {
    let v = js_trim(v);
    let c = super::prefs::parse_hex(&if v.starts_with('#') { v } else { format!("#{v}") })?;
    Some(format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue()))
}

fn open_name(app: &Rc<App>, n: NameForm) {
    st(|st| {
        st.name = Some(n);
        st.focus_tick += 1;
    });
    redraw(app);
}

/// The New Island dialog (name + colour).
pub fn new_island(app: &Rc<App>, parent_id: Option<String>, cell: Option<(f64, f64)>) {
    let color = ISLAND_COLORS[0].to_string();
    open_name(app, NameForm { target: None, parent_id, cell, text: String::new(), colors: true, hex: color.clone(), color, hex_open: false });
}

/// The rename dialog for item `id` (with the colour row for an island).
pub fn rename(app: &Rc<App>, id: &str) {
    let Some(e) = find(app, id) else { return };
    let island = e.str("kind") == Some("island");
    let color = e.str("color").filter(|c| !c.is_empty()).unwrap_or(ISLAND_COLORS[0]).to_string();
    open_name(
        app,
        NameForm {
            target: Some((id.to_string(), island)),
            parent_id: None,
            cell: None,
            text: e.str("name").unwrap_or("").to_string(),
            colors: island,
            hex: color.clone(),
            color,
            hex_open: false,
        },
    );
}

fn save_name(app: &Rc<App>) {
    let Some(n) = st(|st| st.name.take_if(|n| !js_trim(&n.text).is_empty())) else { return };
    refocus(app);
    let value = js_trim(&n.text);
    let res = app.mutate(|apps| {
        match &n.target {
            None => {
                let id = store::new_id();
                items::add_island(apps, id.clone(), &value, n.parent_id.as_deref().unwrap_or(""), &n.color, time::now_ms());
                if let Some((x, y)) = n.cell {
                    items::set_position(apps, &id, x, y);
                }
            }
            Some((id, island)) => {
                items::rename(apps, id, &value);
                if *island {
                    items::set_color(apps, id, &n.color);
                }
            }
        }
        Ok(())
    });
    if let Err(e) = res {
        alert_failed(app, &e);
    }
}

// ---------- move to island ----------

/// getPathLabel: "Outer / Inner", or "Home".
fn path_label(apps: &[Entry], island_id: &str) -> String {
    let mut chain = Vec::new();
    let mut cur = Some(island_id.to_string());
    while let Some(id) = cur.take() {
        let Some(e) = apps.iter().find(|a| a.str("id") == Some(id.as_str()) && a.str("kind") == Some("island")) else { break };
        chain.push(e.str("name").unwrap_or("").to_string());
        if chain.len() > apps.len() {
            break; // a parent loop in a hand-edited file
        }
        cur = e.str("parentId").filter(|p| !p.is_empty()).map(str::to_string);
    }
    chain.reverse();
    if chain.is_empty() {
        "Home".into()
    } else {
        chain.join(" / ")
    }
}

/// The rows of the Move dialog: Home, then the islands it may go into, by name.
fn move_rows(apps: &[Entry], ids: &[String]) -> Vec<(String, Option<String>, bool)> {
    let single = (ids.len() == 1).then(|| apps.iter().find(|a| a.str("id") == Some(ids[0].as_str()))).flatten();
    let current = single.map(|e| e.str("parentId").filter(|p| !p.is_empty()).map(str::to_string));
    let excluded = match single {
        Some(e) if e.str("kind") == Some("island") => items::descendant_island_ids(apps, &ids[0]),
        _ => Default::default(),
    };
    let mut islands: Vec<&Entry> = apps
        .iter()
        .filter(|a| a.str("kind") == Some("island"))
        .filter(|a| a.str("id").is_some_and(|id| !ids.iter().any(|i| i == id) && !excluded.contains(id)))
        .collect();
    // shortcut: lower-cased code-point order stands in for localeCompare.
    islands.sort_by_key(|a| a.str("name").unwrap_or("").to_lowercase());
    let mut rows = vec![("Top Level (Home)".to_string(), None, current == Some(None))];
    for f in islands {
        let id = f.str("id").unwrap_or("").to_string();
        let is_current = current.as_ref().is_some_and(|c| c.as_deref() == Some(id.as_str()));
        rows.push((path_label(apps, &id), Some(id), is_current));
    }
    rows
}

/// The Move to island dialog, moving every id in `ids` at once.
pub fn move_items(app: &Rc<App>, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let rows = move_rows(&app.apps.borrow(), &ids);
    st(|st| {
        st.moving = Some(MoveForm { ids, rows });
        st.focus_tick += 1;
    });
    redraw(app);
}

fn move_pick(app: &Rc<App>, i: usize) {
    let Some((ids, parent)) = st(|st| {
        let m = st.moving.as_ref()?;
        let (_, parent, current) = m.rows.get(i)?;
        if *current {
            return None;
        }
        let r = (m.ids.clone(), parent.clone());
        st.moving = None;
        Some(r)
    }) else {
        return;
    };
    refocus(app);
    let res = app.mutate(|apps| {
        for id in &ids {
            items::move_item(apps, id, parent.as_deref().unwrap_or(""), None);
        }
        Ok(())
    });
    if let Err(e) = res {
        alert_failed(app, &e);
    }
}

// ---------- test hook ----------

/// `LAUNCHPAD_DEBUG_OPEN` (debug builds only): steps run one after another
/// at start, separated by `|`, so a test can reach a modal before the parts
/// that open them exist, and stand in for the system file dialogs (which a
/// test can't drive). `settings[:section]`, `add`, `edit:<id>`, `island`,
/// `rename:<id>`, `move:<id>[,<id>…]`, `alert:<text>`, `confirm:<text>`,
/// `pick:<path>` (Browse File…), `icon:<path>` (Choose Icon…), and
/// Settings' `wallpaper:<path>`, `export:<file>`, `import:<file>`.
#[cfg(debug_assertions)]
fn debug_open(app: &Rc<App>, what: &str) {
    let (step, rest) = what.split_once('|').unwrap_or((what, ""));
    let (verb, arg) = step.split_once(':').unwrap_or((step, ""));
    match verb {
        "settings" => super::settings::open(app, Some(arg).filter(|a| !a.is_empty())),
        "add" => add_app(app, None, None),
        "edit" => edit_app(app, arg),
        "island" => new_island(app, None, None),
        "rename" => rename(app, arg),
        "move" => move_items(app, arg.split(',').map(str::to_string).collect()),
        "alert" => alert(app, arg),
        "confirm" => confirm(app, arg, "OK", false, |app| alert(app, "confirmed")),
        "pick" => fill_target(app, PathBuf::from(arg)),
        "icon" => set_custom_icon(app, PathBuf::from(arg)),
        _ => super::settings::debug_step(app, verb, arg),
    }
    if !rest.is_empty() {
        let rest = rest.to_string();
        slint::Timer::single_shot(std::time::Duration::from_millis(700), move || {
            super::with(|app| debug_open(app, &rest));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn list(v: serde_json::Value) -> Vec<Entry> {
        v.as_array().unwrap().iter().map(|e| Entry::from_value(e.clone()).unwrap()).collect()
    }

    #[test]
    fn move_rows_mark_the_current_parent_and_leave_out_the_island_itself_and_its_insides() {
        let apps = list(json!([
            { "id": "a", "kind": "island", "name": "b-outer", "parentId": null },
            { "id": "b", "kind": "island", "name": "Inner", "parentId": "a" },
            { "id": "c", "kind": "island", "name": "apps", "parentId": null },
            { "id": "x", "kind": "app", "name": "X", "parentId": "b" },
        ]));
        let labels = |rows: Vec<(String, Option<String>, bool)>| rows.into_iter().map(|(l, _, c)| format!("{l}{}", if c { "*" } else { "" })).collect::<Vec<_>>();
        assert_eq!(labels(move_rows(&apps, &["x".into()])), ["Top Level (Home)", "apps", "b-outer", "b-outer / Inner*"]);
        assert_eq!(labels(move_rows(&apps, &["a".into()])), ["Top Level (Home)*", "apps"]);
        assert_eq!(labels(move_rows(&apps, &["a".into(), "x".into()])), ["Top Level (Home)", "apps", "b-outer / Inner"]);
    }

    #[test]
    fn form_data_follows_the_submit_handler() {
        let f = Form { name: " N ".into(), type_: 2, target: " s.py ".into(), args: " -x ".into(), cwd_script: "C:\\w".into(), url: "u".into(), ..Default::default() };
        let d = form_data(&f);
        assert_eq!((d.name.as_str(), d.target.as_str(), d.args.as_str(), d.cwd.as_str(), d.interpreter.as_str()), ("N", "s.py", "-x", "C:\\w", "python"));
        let d = form_data(&Form { type_: 1, url: " https://a ".into(), target: "t".into(), args: "a".into(), ..Default::default() });
        assert_eq!((d.type_.as_str(), d.target.as_str(), d.args.as_str()), ("url", "https://a", ""));
        assert_eq!(normalize_hex("#ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("abc").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("#abcd"), None);
        assert_eq!(js_trim("\u{feff} a "), "a");
    }
}
