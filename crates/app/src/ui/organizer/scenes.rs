//! Scenes (lib/scene-runner.js and main.js's scene handlers in 1.2): saved
//! window arrangements, kept in layouts.json. Saved and applied from a
//! panel's scenes menu, the Scenes dialog in the main window, and an
//! island's right-click menu ("Save arrangement for this island", "Open as
//! scene"). Positions are kept as fractions of the display's work area, so
//! they work in physical pixels as well as 1.2's DIP.
use super::{monitor_at, rect, to_dip, to_rect};
use crate::ui::{post, App};
use crate::win::display::{self, Monitor};
use crate::win::{launch, windowmgr};
use launchpad_core::ids::new_uuid;
use launchpad_core::items;
use launchpad_core::organizer::layouts as core_layouts;
use launchpad_core::organizer::scenes::{self as core_scenes, Display, SavedDisplay, Scene, SceneApp, SceneWindow};
use launchpad_core::organizer::{OpenWindow, Point};
use launchpad_core::store::Entry;
use launchpad_core::time;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// Opening an island as a scene: how long to wait for the apps it launched
// to show a window, how often to look, and how long to let a new window
// settle (apps often move themselves just after showing) before placing
// it, and once more after that.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const LAUNCH_POLL: Duration = Duration::from_millis(500);
const SETTLE: Duration = Duration::from_millis(1000);
const SECOND_PASS: Duration = Duration::from_millis(1500);
const MAX_WINDOWS: usize = 64;

const NOTHING_OPEN: &str = "There are no app windows open to save. Open and arrange some first.";
const ISLAND_NOTHING_OPEN: &str = "None of this island's apps has a window open. Open them, arrange them, then save.";
const BUSY_MSG: &str = "A scene is already being arranged. Try again in a moment.";
const GONE: &str = "That scene no longer exists.";
const NO_ARRANGEMENT: &str = "This island has no saved arrangement yet. Arrange its apps, then use \"Save arrangement for this island\".";
const ISLAND_GONE: &str = "That island no longer exists.";
const NO_PROGRAMS: &str =
    "None of this island's apps is a program Launchpad can find windows for (websites, scripts and commands can't be placed).";

static BUSY: AtomicBool = AtomicBool::new(false);

/// One of the user's items, as much as naming and launching need (Send).
#[derive(Clone)]
struct AppInfo {
    kind: String,
    type_: String,
    target: String,
    name: String,
    parent: String,
}

impl AppInfo {
    fn of(e: &Entry) -> AppInfo {
        let s = |k| e.str(k).unwrap_or("").to_owned();
        AppInfo { kind: s("kind"), type_: s("type"), target: s("target"), name: s("name"), parent: s("parentId") }
    }
    fn scene_app(&self) -> SceneApp<'_> {
        SceneApp { kind: &self.kind, type_: &self.type_, target: &self.target, name: &self.name }
    }
}

fn app_infos(app: &App) -> Vec<AppInfo> {
    app.apps.borrow().iter().map(AppInfo::of).collect()
}

fn namer(apps: &[AppInfo]) -> impl Fn(&str) -> String + '_ {
    move |exe| {
        let list: Vec<SceneApp> = apps.iter().map(AppInfo::scene_app).collect();
        core_scenes::app_name(exe, &list)
    }
}

#[derive(Default)]
pub struct Outcome {
    launched: usize,
    placed: usize,
    missing: Vec<String>,
    failed: Vec<String>,
    not_started: Vec<String>,
}

/// "Placed 3 windows. Not open: Spotify." for a panel message or a dialog.
pub fn describe(o: &Outcome) -> String {
    let s = |n: usize| if n == 1 { "" } else { "s" };
    let mut parts = Vec::new();
    if o.launched > 0 {
        parts.push(format!("Opened {} app{}.", o.launched, s(o.launched)));
    }
    parts.push(format!("Placed {} window{}.", o.placed, s(o.placed)));
    if !o.not_started.is_empty() {
        parts.push(format!("Couldn't open: {}.", o.not_started.join(", ")));
    }
    if !o.missing.is_empty() {
        parts.push(format!("Not open: {}.", o.missing.join(", ")));
    }
    if !o.failed.is_empty() {
        parts.push(format!("Couldn't move: {}.", o.failed.join(", ")));
    }
    parts.join(" ")
}

/// Held while a scene is being arranged; a second one is turned away.
struct Busy;

impl Busy {
    fn take() -> Result<Busy, String> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err(BUSY_MSG.into());
        }
        Ok(Busy)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn saved_display(m: &Monitor) -> SavedDisplay {
    SavedDisplay { id: m.id as f64, bounds: to_dip(m, rect(&m.bounds)) }
}

/// What a scene remembers about an open window. Minimized ones aren't part
/// of the arrangement on screen, so they're left out.
fn capture(w: &windowmgr::Window) -> Option<SceneWindow> {
    if w.minimized {
        return None;
    }
    let f = w.frame;
    let m = monitor_at(Point { x: ((f.left + f.right) / 2) as f64, y: ((f.top + f.bottom) / 2) as f64 })?;
    Some(SceneWindow {
        exe: w.exe.clone()?,
        title: w.title.chars().take(1024).collect(),
        display: saved_display(&m),
        rect: if w.maximized {
            core_scenes::FracRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }
        } else {
            core_scenes::to_fractions(rect(&f), rect(&m.work))
        },
        maximized: w.maximized,
        extra: Default::default(),
    })
}

/// Saves where the open windows are now. `id`: over that scene (keeping its
/// name and island). `island`: as that island's scene, holding only the
/// windows of its apps' exes (and replacing the island's last one).
/// Otherwise a new scene, named `name` or "Scene n".
pub fn save(app: &App, id: Option<&str>, name: Option<String>, island: Option<(String, Vec<String>)>) -> Result<Scene, String> {
    let wanted: Option<HashSet<String>> = island.as_ref().map(|(_, exes)| exes.iter().map(|e| core_scenes::exe_key(e)).collect());
    let windows: Vec<SceneWindow> = windowmgr::windows()
        .iter()
        .filter(|w| wanted.as_ref().is_none_or(|set| w.exe.as_deref().is_some_and(|e| set.contains(&core_scenes::exe_key(e)))))
        .filter_map(capture)
        .take(MAX_WINDOWS)
        .collect();
    if windows.is_empty() {
        return Err(if island.is_some() { ISLAND_NOTHING_OPEN } else { NOTHING_OPEN }.into());
    }
    let all = core_layouts::load(&app.data_dir).scenes;
    let island_id = island.map(|(i, _)| i);
    let existing = match (id, &island_id) {
        (Some(id), _) => Some(all.iter().find(|s| s.id == id).ok_or(GONE)?),
        (None, Some(i)) => all.iter().find(|s| s.island_id.as_deref() == Some(i)),
        _ => None,
    };
    let scene = Scene {
        id: existing.map(|s| s.id.clone()).unwrap_or_else(new_uuid),
        name: name.or_else(|| existing.map(|s| s.name.clone())).unwrap_or_else(|| core_scenes::next_scene_name(&all)),
        island_id: island_id.or_else(|| existing.and_then(|s| s.island_id.clone())),
        saved_at: Some(time::now_ms() as f64),
        windows,
        extra: existing.map(|s| s.extra.clone()).unwrap_or_default(),
    };
    core_layouts::save_scene(&app.data_dir, &scene).map_err(|e| e.to_string())?;
    super::dialogs::scenes_changed(app);
    Ok(scene)
}

/// Saving over a scene saved from an island again holds only that island's apps.
pub fn save_over(app: &App, id: &str) -> Result<Scene, String> {
    let scene = core_layouts::load(&app.data_dir).scenes.into_iter().find(|s| s.id == id).ok_or(GONE)?;
    let apps = app_infos(app);
    let island = scene.island_id.as_ref().filter(|i| app.apps.borrow().iter().any(|e| e.str("id") == Some(i) && e.str("kind") == Some("island")));
    match island {
        Some(i) => save(app, Some(id), None, Some((i.clone(), island_exes(&apps, i)))),
        None => save(app, Some(id), None, None),
    }
}

fn island_exes(apps: &[AppInfo], island: &str) -> Vec<String> {
    apps.iter().filter(|a| a.parent == island).filter_map(|a| core_scenes::app_exe(&a.scene_app()).map(str::to_owned)).collect()
}

/// "Save arrangement for this island": Ok is the message to show.
pub fn save_island(app: &App, island: &str) -> Result<String, String> {
    let name = app
        .apps
        .borrow()
        .iter()
        .find(|e| e.str("id") == Some(island) && e.str("kind") == Some("island"))
        .map(|e| e.str("name").unwrap_or("").to_owned())
        .ok_or(ISLAND_GONE)?;
    let exes = island_exes(&app_infos(app), island);
    if exes.is_empty() {
        return Err(NO_PROGRAMS.into());
    }
    let scene = save(app, None, Some(name.clone()), Some((island.to_owned(), exes)))?;
    let n = scene.windows.len();
    let count = if n == 1 { "1 window".to_string() } else { format!("{n} windows") };
    Ok(format!("Saved the arrangement for \"{name}\" ({count}). Right-click the island and choose \"Open as scene\" to bring it back."))
}

fn open_windows() -> Vec<OpenWindow> {
    windowmgr::windows().iter().map(|w| w.open_window()).collect()
}

fn dedupe(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in v {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Moves every open window the scene has a place for back there. Any thread.
fn place_scene(scene: &Scene, name: &dyn Fn(&str) -> String) -> Outcome {
    let open = open_windows();
    let matches = core_scenes::match_windows(&scene.windows, &open);
    let monitors = display::monitors();
    // Bounds in DIP for matching what 1.2 saved; work areas physical, for placing.
    let displays: Vec<Display> =
        monitors.iter().map(|m| Display { id: m.id as f64, bounds: to_dip(m, rect(&m.bounds)), work_area: rect(&m.work) }).collect();
    let mut out = Outcome::default();
    let Some(primary) = monitors.iter().position(|m| m.primary).or((!displays.is_empty()).then_some(0)).map(|i| displays[i]) else {
        return out;
    };
    for pair in &matches.matched {
        let d = core_scenes::pick_display(&pair.entry.display, &displays, &primary);
        let r = to_rect(core_scenes::from_fractions(pair.entry.rect, d.work_area));
        match windowmgr::place(pair.window.hwnd, r, pair.entry.maximized) {
            Ok(_) => out.placed += 1,
            Err(_) => out.failed.push(name(&pair.entry.exe)),
        }
    }
    out.missing = dedupe(matches.missing.iter().map(|e| name(&e.exe)).collect());
    out.failed = dedupe(out.failed);
    out
}

/// Applies a scene: Ok is the message saying what was placed.
pub fn apply(app: &App, id: &str) -> Result<String, String> {
    let _busy = Busy::take()?;
    let scene = core_layouts::load(&app.data_dir).scenes.into_iter().find(|s| s.id == id).ok_or(GONE)?;
    let apps = app_infos(app);
    let name = namer(&apps);
    let out = place_scene(&scene, &name);
    Ok(describe(&out))
}

/// "Open as scene" on an island: launches each of the island's apps that the
/// scene has a window for and that has none open now, waits for their
/// windows, and applies the scene. Off the UI thread; `done` gets the message.
pub fn open_island(app: &Rc<App>, island: &str, done: fn(&Rc<App>, String)) {
    let busy = match Busy::take() {
        Ok(b) => b,
        Err(e) => return done(app, e),
    };
    let Some(scene) = core_layouts::load(&app.data_dir).scenes.into_iter().find(|s| s.island_id.as_deref() == Some(island)) else {
        return done(app, NO_ARRANGEMENT.into());
    };
    let apps = app_infos(app);
    let in_scene: HashSet<String> = scene.windows.iter().map(|w| core_scenes::exe_key(&w.exe)).collect();
    let running: HashSet<String> = open_windows().iter().map(|w| core_scenes::exe_key(&w.exe)).collect();
    let mut to_launch: Vec<(String, String, Entry)> = Vec::new(); // exe key, id, item
    for e in app.apps.borrow().iter() {
        let info = AppInfo::of(e);
        let Some(exe) = core_scenes::app_exe(&info.scene_app()) else { continue };
        let key = core_scenes::exe_key(exe);
        if info.parent == island && in_scene.contains(&key) && !running.contains(&key) && !to_launch.iter().any(|t| t.0 == key) {
            to_launch.push((key, e.str("id").unwrap_or("").to_owned(), e.clone()));
        }
    }
    std::thread::spawn(move || {
        let _busy = busy;
        let _com = crate::win::Com::init();
        let mut launched: Vec<String> = Vec::new();
        let mut not_started: Vec<String> = Vec::new();
        let mut recorded: Vec<String> = Vec::new();
        for (key, item_id, entry) in &to_launch {
            match launch::launch_item(&entry.0) {
                Ok(()) => {
                    launched.push(key.clone());
                    recorded.push(item_id.clone());
                }
                Err(_) => not_started.push(entry.str("name").unwrap_or("").to_owned()),
            }
        }
        if !recorded.is_empty() {
            post(move |app| {
                let now = time::now_ms();
                let _ = app.mutate(|list| {
                    for id in &recorded {
                        items::record_launch(list, id, now);
                    }
                    Ok(())
                });
            });
        }
        if !launched.is_empty() {
            let deadline = Instant::now() + LAUNCH_TIMEOUT;
            while Instant::now() < deadline {
                std::thread::sleep(LAUNCH_POLL);
                let open = open_windows();
                let m = core_scenes::match_windows(&scene.windows, &open);
                if !m.missing.iter().any(|e| launched.contains(&core_scenes::exe_key(&e.exe))) {
                    break;
                }
            }
            std::thread::sleep(SETTLE);
        }
        let name = namer(&apps);
        let mut out = place_scene(&scene, &name);
        if !launched.is_empty() {
            std::thread::sleep(SECOND_PASS);
            out = place_scene(&scene, &name);
        }
        // Apps that couldn't be started are named once, not again as "not open".
        out.missing.retain(|n| !not_started.contains(n));
        out.launched = launched.len();
        out.not_started = not_started;
        let msg = describe(&out);
        post(move |app| done(app, msg));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_matches_1_2() {
        let o = Outcome { launched: 1, placed: 3, missing: vec!["Spotify".into()], failed: vec![], not_started: vec!["X".into()] };
        assert_eq!(describe(&o), "Opened 1 app. Placed 3 windows. Couldn't open: X. Not open: Spotify.");
        assert_eq!(describe(&Outcome { placed: 1, ..Default::default() }), "Placed 1 window.");
    }
}
