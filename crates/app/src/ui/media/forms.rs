//! Media's Add screens (a single app, a library folder) and Customize art,
//! with main.js's checks: libraryRequest, media:addApp's duplicate check,
//! setEntryArt.
use super::{art_of, image, library, md, reload, render_home, set_focus, settings, show_screen, st, toast, App};
use crate::ui::{post, MGroup, MSlot};
use crate::win::{dialogs, image::Wic, services};
use launchpad_core::media::art as core_art;
use launchpad_core::media::library::{self as lib, Entry};
use launchpad_core::media::scan::{self, ScanOptions, Summary};
use launchpad_core::time::now_ms;
use serde_json::{json, Map, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use windows::Win32::Foundation::HWND;

const LIBRARIES: [(&str, &str); 2] = [("games", "Games"), ("videos", "Videos")];
const SLOTS: [(&str, &str, &str); 4] = [
    ("cover", "Cover", "Portrait box art, shown on game tiles"),
    ("banner", "Banner", "Wide 16:9 art, shown on video tiles and behind the library"),
    ("logo", "Logo", "Title logo with a transparent background, shown instead of the name"),
    ("screenshot", "Screenshot", "A still from the game or video"),
];

fn lib_label(l: &str) -> &'static str {
    LIBRARIES.iter().find(|(k, _)| *k == l).map_or("Games", |(_, v)| v)
}

#[derive(Default)]
pub struct Add {
    root: Option<String>,
    library: &'static str,
    recursive: bool,
    platforms: Map<String, Value>,
    groups: Vec<Summary>,
    run: u32,
}

#[derive(Default)]
pub struct AddApp {
    target: Option<String>,
    library: &'static str,
    /// A platform id, or "" for Other games.
    platform: String,
}

fn owner() -> Option<HWND> {
    md(|m| m.hwnd).flatten()
}

pub fn activate(app: &Rc<App>, kind: &str, rest: &str) {
    match (kind, rest) {
        ("choice", "single") => open_add_app(app),
        ("choice", "folder") => open_add(app),
        ("app", "browse") => browse_app(app),
        ("app", "library") => pick(app, "app:library", cycle(&["games", "videos"], md(|m| m.app_form.library).unwrap_or("games"))),
        ("app", "platform") => {
            let cur = md(|m| m.app_form.platform.clone()).unwrap_or_default();
            let i = scan::PLATFORMS.iter().position(|p| p.id == cur).map_or(0, |i| i + 1);
            pick(app, "app:platform", (i + 1) % (scan::PLATFORMS.len() + 1));
        }
        ("app", "add") => add_app(app),
        ("add", "browse") => browse_folder(app),
        ("add", "type") => pick(app, "add:type", cycle(&["games", "videos"], md(|m| m.add.library).unwrap_or("games"))),
        ("add", "recursive") => {
            md(|m| m.add.recursive = !m.add.recursive);
            preview(app);
        }
        ("add", "confirm") => add_folder(app),
        ("group", i) => {
            let i: usize = i.parse().unwrap_or(0);
            let cur = md(|m| m.add.groups.get(i).and_then(|g| g.platform.clone())).flatten();
            let at = cur.and_then(|c| scan::PLATFORMS.iter().position(|p| p.id == c)).map_or(0, |p| p + 1);
            pick(app, &format!("group:{i}"), (at + 1) % (scan::PLATFORMS.len() + 1));
        }
        ("slot-file", slot) => art_from_file(app, slot),
        ("slot-clear", slot) => set_art(app, slot, None),
        ("slot-use", slot) => art_from_url(app, slot),
        _ => {}
    }
}

/// The index after `cur` in `list`, wrapping.
fn cycle(list: &[&str], cur: &str) -> usize {
    (list.iter().position(|x| *x == cur).unwrap_or(0) + 1) % list.len()
}

/// A dropdown's choice (`i` into its options), from the mouse or a cycle.
pub fn pick(app: &Rc<App>, id: &str, i: usize) {
    let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
    let platform = |i: usize| i.checked_sub(1).and_then(|i| scan::PLATFORMS.get(i)).map(|p| p.id);
    match (kind, rest) {
        ("app", "library") => {
            md(|m| m.app_form.library = LIBRARIES[i.min(1)].0);
            render_add_app();
        }
        ("app", "platform") => {
            md(|m| m.app_form.platform = platform(i).unwrap_or("").into());
            render_add_app();
        }
        ("add", "type") => {
            md(|m| {
                m.add.library = LIBRARIES[i.min(1)].0;
                m.add.platforms = Map::new();
            });
            preview(app);
        }
        ("group", g) => {
            let g: usize = g.parse().unwrap_or(0);
            md(|m| {
                let Some(folder) = m.add.groups.get(g).map(|s| s.folder.clone()) else { return };
                match platform(i) {
                    Some(p) => m.add.platforms.insert(folder, p.into()),
                    None => m.add.platforms.remove(&folder),
                };
            });
            preview(app);
        }
        _ => settings::pick(app, kind, rest, i),
    }
    // The dropdown keeps the focus after a pick.
    set_focus(app, id);
}

/// Enter in a text field.
pub fn accepted(app: &Rc<App>, id: &str) {
    let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
    match kind {
        "app" => add_app(app),
        "slot-url" => art_from_url(app, rest),
        _ => settings::accepted(app, kind, rest),
    }
}

// ---------- Add a single app ----------

fn open_add_app(app: &Rc<App>) {
    let (library, _) = settings::library_defaults(app);
    md(|m| {
        m.app_form = AddApp { target: None, library, platform: String::new() };
        let s = st(m);
        s.set_app_exe("".into());
        s.set_app_name("".into());
        s.set_app_error("".into());
    });
    render_add_app();
    show_screen(app, "addApp");
}

fn render_add_app() {
    md(|m| {
        let f = &m.app_form;
        let s = st(m);
        s.set_app_library(lib_label(f.library).into());
        s.set_app_show_platform(f.library == "games");
        s.set_app_platform(scan::platform_by_id(&f.platform).map_or("Other games", |p| p.name).into());
        s.set_app_can_add(f.target.is_some());
        super::bump(m);
    });
}

fn browse_app(app: &Rc<App>) {
    let filters: [dialogs::Filter; 1] = [("Programs", &["exe"])];
    let exe = match dialogs::open_file(owner(), &filters, None) {
        Ok(Some(p)) => p.to_string_lossy().into_owned(),
        Ok(None) => return,
        Err(e) => return toast(&e.to_string()),
    };
    md(|m| {
        let s = st(m);
        s.set_app_exe(exe.as_str().into());
        if s.get_app_name().trim().is_empty() {
            let stem = Path::new(&exe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            s.set_app_name(stem.into());
        }
        s.set_app_error("".into());
        m.app_form.target = Some(exe);
    });
    render_add_app();
    let _ = app;
}

fn add_app(app: &Rc<App>) {
    let Some((target, name, library, platform)) = md(|m| {
        let f = &m.app_form;
        f.target.clone().map(|t| (t, st(m).get_app_name().to_string(), f.library, f.platform.clone()))
    })
    .flatten() else {
        return;
    };
    let request = json!({
        "target": target,
        "name": name,
        "library": library,
        "platform": if library == "games" && !platform.is_empty() { Value::from(platform) } else { Value::Null },
    });
    let platforms: HashSet<String> = scan::PLATFORMS.iter().map(|p| p.id.to_string()).collect();
    let result = library_add(app, |entries| {
        let entry = lib::single_app_entry(&request, |p| Path::new(p).is_file(), &platforms, now_ms())?;
        if let Some(dupe) = entries.iter().find(|e| e.launch_type() == "path" && e.target().to_lowercase() == target.to_lowercase()) {
            return Err(launchpad_core::Error::msg(format!("{} is already in the library", dupe.name())));
        }
        entries.push(entry.clone());
        Ok(entry)
    });
    match result {
        Err(e) => {
            md(|m| st(m).set_app_error(e.to_string().into()));
        }
        Ok(entry) => {
            toast(&format!("Added {}", entry.name()));
            after_add(app, library);
            // The new tile, wherever its row is.
            let nav = md(|m| {
                m.tiles.iter().enumerate().find_map(|(ri, (_, ids, _))| ids.iter().any(|id| id == entry.id()).then(|| format!("tile:{ri}:{}", entry.id())))
            })
            .flatten();
            if let Some(nav) = nav {
                set_focus(app, &nav);
            }
        }
    }
}

/// Loads library.json, lets `f` change the list, and saves it.
fn library_add<R>(app: &App, f: impl FnOnce(&mut Vec<Entry>) -> launchpad_core::Result<R>) -> launchpad_core::Result<R> {
    library(app, |l| {
        let mut entries = l.load()?;
        let r = f(&mut entries)?;
        l.save(&entries)?;
        Ok(r)
    })
}

/// Home on the library just added to.
fn after_add(app: &Rc<App>, library: &'static str) {
    md(|m| {
        m.tab = library;
        m.home_focus = None;
    });
    reload(app);
    show_screen(app, "home");
}

// ---------- Add library folder ----------

fn open_add(app: &Rc<App>) {
    let (library, recursive) = settings::library_defaults(app);
    let profiles = launchpad_core::media::players::load(&app.data_dir).unwrap_or_default();
    md(|m| {
        m.profiles = profiles;
        m.add = Add { root: None, library, recursive, platforms: Map::new(), groups: Vec::new(), run: m.add.run };
        let s = st(m);
        s.set_add_root("".into());
        s.set_add_status("Choose a folder to see what is in it.".into());
    });
    render_add();
    show_screen(app, "add");
}

fn render_add() {
    md(|m| {
        let a = &m.add;
        let videos = a.library == "videos";
        let default_name = |key: &str| {
            let id = m.profiles.defaults.get(key).and_then(Value::as_str)?;
            m.profiles.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone())
        };
        let groups: Vec<MGroup> = a
            .groups
            .iter()
            .map(|g| {
                let key = if videos { Some("videos") } else { g.platform.as_deref() };
                let name = key.and_then(default_name);
                MGroup {
                    folder: if g.folder.is_empty() { "(top folder)".into() } else { format!("{}\\", g.folder).into() },
                    platform: g.platform.as_deref().and_then(scan::platform_by_id).map_or("Not detected", |p| p.name).into(),
                    warn: g.platform.is_none(),
                    count: g.count.to_string().into(),
                    launch_warn: name.is_none(),
                    launch: name.unwrap_or_else(|| if key.is_some() { "Set up in Settings" } else { "Choose a platform" }.into()).into(),
                }
            })
            .collect();
        let s = st(m);
        s.set_add_type(lib_label(a.library).into());
        s.set_add_recursive(a.recursive);
        s.set_add_groups(ModelRc::new(VecModel::from(groups)));
        super::bump(m);
    });
}

fn browse_folder(app: &Rc<App>) {
    let root = match dialogs::open_folder(owner(), None) {
        Ok(Some(p)) => p.to_string_lossy().into_owned(),
        Ok(None) => return,
        Err(e) => return toast(&e.to_string()),
    };
    md(|m| {
        st(m).set_add_root(root.as_str().into());
        m.add.root = Some(root);
        m.add.platforms = Map::new();
    });
    preview(app);
}

/// libraryRequest(): a folder that exists, games or videos, known platforms.
fn request(m: &super::Media) -> Result<(PathBuf, ScanOptions, Value), String> {
    let a = &m.add;
    let root = a.root.clone().unwrap_or_default();
    let p = PathBuf::from(&root);
    if !p.is_absolute() || !p.is_dir() {
        return Err("Choose a folder that exists".into());
    }
    let req = json!({ "root": root, "library": a.library, "recursive": a.recursive, "platforms": a.platforms });
    Ok((p, ScanOptions { library: a.library.into(), recursive: a.recursive, platforms: a.platforms.clone() }, req))
}

fn preview(_app: &Rc<App>) {
    let Some((req, run, videos)) = md(|m| {
        m.add.root.as_ref()?;
        m.add.run += 1;
        let s = st(m);
        s.set_add_status("Looking through the folder…".into());
        s.set_add_can_confirm(false);
        Some((request(m), m.add.run, m.add.library == "videos"))
    })
    .flatten() else {
        render_add();
        return;
    };
    render_add();
    let (root, options, _) = match req {
        Ok(r) => r,
        Err(e) => {
            md(|m| {
                st(m).set_add_status(e.into());
                m.add.groups.clear();
            });
            return render_add();
        }
    };
    std::thread::spawn(move || {
        let found = scan::scan_folder(&root, &options);
        let (count, groups) = (found.len(), scan::summarize(&found));
        post(move |_| {
            let fresh = md(|m| {
                if m.add.run != run {
                    return false; // a newer preview started
                }
                let noun = match (videos, count == 1) {
                    (true, true) => "video",
                    (true, false) => "videos",
                    (false, true) => "game",
                    (false, false) => "games",
                };
                let s = st(m);
                s.set_add_status(if count > 0 { format!("Found {count} {noun}.") } else { format!("No {noun} found in this folder.") }.into());
                s.set_add_can_confirm(count > 0);
                m.add.groups = groups;
                true
            });
            if fresh == Some(true) {
                render_add();
            }
        });
    });
}

fn add_folder(app: &Rc<App>) {
    let Some(req) = md(|m| {
        st(m).set_add_can_confirm(false);
        (request(m), m.add.library)
    }) else {
        return;
    };
    let ((root, options, json_req), library) = match req {
        (Ok(r), l) => (r, l),
        (Err(e), _) => return toast(&e),
    };
    let root_text = root.to_string_lossy().into_owned();
    std::thread::spawn(move || {
        let found = scan::scan_folder(&root, &options);
        post(move |app| {
            let merged = library_add(app, |entries| {
                let r = scan::merge_scan(entries, &found, &root_text, library, now_ms());
                *entries = r.entries;
                Ok((r.added, r.missing, r.restored))
            });
            match merged {
                Err(e) => {
                    toast(&e.to_string());
                    md(|m| st(m).set_add_can_confirm(true));
                }
                Ok((added, missing, restored)) => {
                    // Remembered, so a later rescan uses the same choices.
                    let same = |l: &Value| {
                        l.get("root").and_then(Value::as_str).is_some_and(|r| r.to_lowercase() == root_text.to_lowercase())
                            && l.get("library").and_then(Value::as_str) == Some(library)
                    };
                    let mut libs: Vec<Value> =
                        app.settings().get("libraries").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter(|l| !same(l)).collect();
                    libs.push(json_req.clone());
                    if let Err(e) = app.save_settings(json!({ "libraries": libs })) {
                        eprintln!("Saving the library folder: {e}");
                    }
                    let mut parts = vec![format!("Added {added}")];
                    if restored > 0 {
                        parts.push(format!("{restored} found again"));
                    }
                    if missing > 0 {
                        parts.push(format!("{missing} no longer found"));
                    }
                    toast(&parts.join(", "));
                    after_add(app, library);
                }
            }
        });
    });
    let _ = app;
}

// ---------- Customize art ----------

pub fn open_customize(app: &Rc<App>, id: &str) {
    if !md(|m| m.entries.iter().any(|e| e.id() == id)).unwrap_or(false) {
        return;
    }
    md(|m| {
        m.customizing = Some(id.to_string());
        m.edits.retain(|k, _| !k.starts_with("slot-url:"));
    });
    render_customize(app);
    show_screen(app, "customize");
}

fn source_text(m: Option<&Value>) -> (String, bool) {
    let Some(m) = m.filter(|v| launchpad_core::truthy(Some(v))) else { return ("Empty".into(), false) };
    let source = m.get("source").map_or_else(String::new, |s| s.as_str().map_or_else(|| s.to_string(), str::to_string));
    if source == "user" {
        return ("Set by you. Scrapes will not replace it.".into(), true);
    }
    (format!("From {}", source.strip_prefix("scraper:").unwrap_or(&source)), false)
}

pub fn render_customize(app: &Rc<App>) {
    md(|m| {
        let Some(e) = m.customizing.as_ref().and_then(|id| m.entries.iter().find(|e| e.id() == id)).cloned() else { return };
        let width = m.win.window().size().to_logical(super::scale(m)).width;
        let card = ((width - 80.0 - 20.0) / 2.0 - 36.0).max(200.0);
        let media = e.0.get("media");
        let mut slots: Vec<MSlot> = Vec::new();
        for (slot, label, hint) in SLOTS {
            let (source, user) = source_text(media.and_then(|m| m.get(slot)));
            let cover = slot == "cover";
            let has = art_of(app, &e, slot).is_some();
            let img = if has { image(app, m, &e, &[slot], card, if cover { 240.0 } else { card * 9.0 / 16.0 }, false) } else { None };
            slots.push(MSlot {
                slot: slot.into(),
                label: label.into(),
                hint: hint.into(),
                source: source.into(),
                user,
                has_art: has,
                art: img.unwrap_or_default(),
                cover,
            });
        }
        let s = st(m);
        s.set_cz_title(format!("Customize: {}", e.name()).into());
        let bottom = slots.split_off(2);
        s.set_slots_top(ModelRc::new(VecModel::from(slots)));
        s.set_slots_bottom(ModelRc::new(VecModel::from(bottom)));
        super::bump(m);
    });
    super::start_art();
}

fn art_from_file(app: &Rc<App>, slot: &str) {
    let filters: [dialogs::Filter; 1] = [("Images", &["png", "jpg", "jpeg"])];
    match dialogs::open_file(owner(), &filters, None) {
        Ok(Some(p)) => art_from_path(app, slot, &p),
        Ok(None) => {}
        Err(e) => toast(&e.to_string()),
    }
}

fn art_from_path(app: &Rc<App>, slot: &str, path: &Path) {
    // readImageFile: a real file of a sane size; decoding refuses anything but PNG or JPEG.
    match core_art::read_image_file(path) {
        Ok(buf) => set_art(app, slot, Some(buf)),
        Err(e) => toast(&e.to_string()),
    }
}

fn art_from_url(app: &Rc<App>, slot: &str) {
    let key = format!("slot-url:{slot}");
    let Some(url) = md(|m| m.edits.get(&key).map(|u| u.trim().to_string())).flatten().filter(|u| !u.is_empty()) else { return };
    let slot = slot.to_string();
    toast("Downloading…");
    std::thread::spawn(move || {
        let r = core_art::download_image(&url, &mut services::Http);
        post(move |app| match r {
            Ok(buf) => set_art(app, &slot, Some(buf)),
            Err(e) => toast(&e.to_string()),
        });
    });
    let _ = app;
}

/// Stores (or with None clears) one slot's art for the entry being customized.
fn set_art(app: &Rc<App>, slot: &str, buf: Option<Vec<u8>>) {
    let Some(id) = md(|m| m.customizing.clone()).flatten() else { return };
    let r = Wic::new().and_then(|wic| library(app, |l| core_art::set_entry_art(l, &id, slot, buf.as_deref(), "user", &mut services::Art(wic))));
    match r {
        Err(e) => toast(&e.to_string()),
        Ok(updated) => {
            md(|m| {
                if let Some(e) = m.entries.iter_mut().find(|e| e.id() == updated.id()) {
                    *e = updated;
                }
            });
            if buf.is_some() {
                toast("Art saved");
            }
            render_customize(app);
            render_home(app);
            set_focus(app, &format!("slot-file:{slot}"));
        }
    }
}

// ---------- Files dropped from Explorer ----------

pub fn register_drop(hwnd: HWND) {
    let on_drop: crate::ui::grid::drop::OnDrop = |paths, _alt, x, y| {
        crate::ui::later(move |app| dropped(app, paths, x, y));
    };
    if let Err(e) = crate::ui::grid::drop::register(hwnd, on_drop, |_| {}) {
        eprintln!("Media's drop target: {e}");
    }
}

/// A file dropped on Customize's art slots (client px).
fn dropped(app: &Rc<App>, paths: Vec<String>, x: i32, y: i32) {
    let slot = md(|m| {
        if m.screen != "customize" {
            return None;
        }
        let s = super::scale(m);
        let (x, y) = (x as f32 / s, y as f32 / s);
        m.rects
            .iter()
            .filter(|(_, (_, e))| *e == m.epoch)
            .find(|(id, (r, _))| id.starts_with("drop:") && x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
            .map(|(id, _)| id["drop:".len()..].to_string())
    })
    .flatten();
    if let (Some(slot), Some(path)) = (slot, paths.first()) {
        art_from_path(app, &slot, Path::new(path));
    }
}
