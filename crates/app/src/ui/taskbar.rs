//! See ui/taskbar.slint. The pinned row, the open windows, search, Recent,
//! the bar's own menu. Lists and their order come from `core::taskbar`.
//!
//! Open windows: 1.2 polled the list every 3 s. Here the list is read when
//! the foreground window changes (or its title does), from the window
//! events hook, which runs only while the main window exists. A window that
//! opens or closes without the foreground changing shows up at the next
//! change (or when its stale button is clicked).
use super::{modals, post, App, MainWindow, TaskbarItem, TaskbarState, TaskbarWindow};
use crate::win::image::Wic;
use crate::win::{launch, windowmgr};
use launchpad_core::store::Entry;
use launchpad_core::{items, taskbar as core_taskbar, time};
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

/// ISLAND_COLORS[0]: an island without a colour of its own.
const ISLAND_DEFAULT: slint::Color = slint::Color::from_rgb_u8(0x5b, 0x8c, 0xff);

#[derive(Default)]
struct Windows {
    /// As shown: (hwnd, title, exe).
    list: Vec<(i64, String, Option<String>)>,
    /// Exe icons, None when the exe has none. Dropped with the window (bind).
    icons: HashMap<String, Option<Image>>,
    busy: bool,
    again: bool,
}

thread_local! {
    static WINDOWS: RefCell<Windows> = RefCell::new(Windows::default());
}

/// Wires the global's callbacks on a newly made window.
pub fn bind(app: &Rc<App>, w: &MainWindow) {
    WINDOWS.with(|s| *s.borrow_mut() = Windows::default());
    let t = w.global::<TaskbarState>();
    t.on_open_item(|id| {
        super::with(|app| open_item(app, &id));
    });
    t.on_pin_dropped(|from, gap| {
        super::with(|app| {
            let displayed: Vec<String> = pinned_ids(app);
            let order = core_taskbar::reorder(&displayed, from.max(0) as usize, gap.max(0) as usize, mirrored(app));
            let ids: Vec<&str> = order.iter().map(String::as_str).collect();
            if let Err(e) = app.mutate(|apps| {
                items::set_pin_order(apps, &ids);
                Ok(())
            }) {
                modals::alert_failed(app, &e);
            }
        });
    });
    t.on_pin_menu(|action, id| {
        let id = id.to_string();
        // After the menu has closed: the dialogs it opens replace the window's state.
        super::later(move |app| pin_menu(app, &action, &id));
    });
    t.on_focus_window(|i| {
        super::with(|app| {
            let hwnd = WINDOWS.with(|s| s.borrow().list.get(i.max(0) as usize).map(|w| w.0));
            if let Some(h) = hwnd {
                if windowmgr::focus(h).is_err() {
                    refresh_windows(app); // it's gone
                }
            }
        });
    });
    t.on_search(|q| {
        super::with(|app| {
            let w = app.window.borrow().as_ref().map(|w| w.clone_strong());
            w.is_some_and(|w| push_results(app, &w, &q))
        })
        .unwrap_or(false)
    });
    t.on_show_recent(|| {
        super::with(|app| {
            if let Some(w) = app.window.borrow().as_ref() {
                push_recent(app, w);
            }
        });
    });
    t.on_set_pref(|key, value| {
        let (key, value) = (key.to_string(), value.to_string());
        // Later: an edge change replaces the taskbar this callback runs in.
        super::later(move |app| {
            app.set_pref(&key, Some(value.as_str()).filter(|v| !v.is_empty()));
            app.refresh();
        });
    });
    t.on_open_settings(|| super::later(|app| super::settings::open(app, None)));

    let events = windowmgr::events::start(|ev| {
        if matches!(ev, windowmgr::events::WmEvent::Foreground(_) | windowmgr::events::WmEvent::WindowsChanged) {
            post(refresh_windows);
        }
    })
    .map_err(|e| eprintln!("Open windows: {e}"))
    .ok();
    if events.is_none() {
        refresh_windows(app);
    }
    // The hook lives as long as this window: it's dropped with the callback.
    t.on_keepalive(move || {
        let _ = &events;
    });
}

/// Pushes the current state into the window.
pub fn refresh(app: &App, w: &MainWindow) {
    let t = w.global::<TaskbarState>();
    t.set_center(app.pref("launchpad-taskbar-align").as_deref() == Some("center"));
    t.set_mirrored(mirrored(app));
    let tray = app.pref("launchpad-tray-align").filter(|a| a == "center" || a == "shared");
    t.set_tray_align(tray.unwrap_or("edge".into()).into());

    let px = px(w, 26.0);
    let apps = app.apps.borrow();
    let pinned: Vec<TaskbarItem> = core_taskbar::pinned(&apps, mirrored(app)).into_iter().map(|e| item(app, &apps, e, px, false)).collect();
    drop(apps);
    t.set_pinned(model(pinned));
    push_windows(app, w);
    // An open popout shows what changed (a launch lands in Recent).
    push_results(app, w, &t.get_search_text());
    push_recent(app, w);
}

/// Type-anywhere: puts `text` at the end of the search box, focuses it and
/// searches, as 1.2's keydown handler did (for the grid's keyboard).
pub fn start_search(app: &Rc<App>, text: &str) {
    let Some(w) = app.window.borrow().as_ref().map(|w| w.clone_strong()) else { return };
    let t = w.global::<TaskbarState>();
    let q = format!("{}{text}", t.get_search_text());
    t.set_search_text(q.as_str().into());
    t.set_focus_search(t.get_focus_search() + 1);
    t.set_current(0);
    let open = push_results(app, &w, &q);
    w.global::<super::Popouts>().set_open(if open { "search".into() } else { SharedString::new() });
}

fn mirrored(app: &App) -> bool {
    app.pref("launchpad-taskbar-position").as_deref() == Some("right")
}

fn pinned_ids(app: &App) -> Vec<String> {
    core_taskbar::pinned(&app.apps.borrow(), mirrored(app)).iter().filter_map(|e| e.str("id").map(String::from)).collect()
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// `logical` px in physical pixels, for decoding icons at draw size.
fn px(w: &MainWindow, logical: f32) -> u32 {
    (logical * w.window().scale_factor()).round().max(1.0) as u32
}

// iconNode() in renderer.js.
fn item(app: &App, apps: &[Entry], e: &Entry, px: u32, with_path: bool) -> TaskbarItem {
    let island = e.str("kind") == Some("island");
    let stored = e.str("type") == Some("stored");
    let mut icon = Image::default();
    let kind = if island {
        "island"
    } else if stored && e.str("storedKind") == Some("folder") {
        "stored-folder"
    } else if e.str("special") == Some("recycleBin") && !e.truthy("iconIsCustom") {
        "recycle"
    } else if let Some(img) = e.str("iconPath").filter(|p| !p.is_empty()).and_then(|p| app.images.icon(Path::new(p), px)) {
        icon = img;
        "image"
    } else {
        "placeholder"
    };
    TaskbarItem {
        id: e.str("id").unwrap_or("").into(),
        name: e.str("name").unwrap_or("").into(),
        path: if with_path { core_taskbar::path_label(apps, e.str("parentId")).into() } else { SharedString::new() },
        kind: kind.into(),
        icon,
        color: e.str("color").and_then(super::prefs::parse_hex).unwrap_or(ISLAND_DEFAULT),
        app: !island,
        stored,
    }
}

/// Fills the search results; false for a blank query (the popout stays shut).
fn push_results(app: &App, w: &MainWindow, q: &str) -> bool {
    let px = px(w, 28.0);
    let apps = app.apps.borrow();
    let found: Vec<TaskbarItem> = core_taskbar::search(&apps, q).into_iter().map(|e| item(app, &apps, e, px, true)).collect();
    drop(apps);
    w.global::<TaskbarState>().set_results(model(found));
    !core_taskbar::is_blank(q)
}

fn push_recent(app: &App, w: &MainWindow) {
    let px = px(w, 28.0);
    let apps = app.apps.borrow();
    let recent: Vec<TaskbarItem> = core_taskbar::recent(&apps).into_iter().map(|e| item(app, &apps, e, px, true)).collect();
    drop(apps);
    w.global::<TaskbarState>().set_recent(model(recent));
}

fn open_item(app: &Rc<App>, id: &str) {
    let entry = app.apps.borrow().iter().find(|a| a.str("id") == Some(id)).cloned();
    match entry {
        Some(e) if e.str("kind") == Some("island") => super::grid::open_island(app, Some(id.to_string())),
        Some(_) => launch_app(app, id),
        None => {}
    }
}

/// launchApp: off the UI thread, then recorded for Recent.
pub fn launch_app(app: &Rc<App>, id: &str) {
    let entry = match items::launchable(&app.apps.borrow(), id) {
        Ok(e) => e.clone(),
        Err(e) => return modals::alert(app, &format!("Couldn't launch:\n{e}")),
    };
    let (id, name) = (id.to_string(), entry.str("name").unwrap_or("").to_string());
    std::thread::spawn(move || {
        let r = launch::launch_item(&entry.0);
        post(move |app| match r {
            Ok(()) => {
                if let Err(e) = app.mutate(|apps| {
                    items::record_launch(apps, &id, time::now_ms());
                    Ok(())
                }) {
                    eprintln!("Recording the launch: {e}");
                }
            }
            Err(e) => modals::alert(app, &format!("Couldn't launch \"{name}\":\n{e}")),
        });
    });
}

fn pin_menu(app: &Rc<App>, action: &str, id: &str) {
    match action {
        "rename" => modals::rename(app, id),
        "edit" => modals::edit_app(app, id),
        "move" => modals::move_items(app, vec![id.to_string()]),
        "unpin" => {
            if let Err(e) = app.mutate(|apps| {
                items::toggle_pin(apps, id);
                Ok(())
            }) {
                modals::alert_failed(app, &e);
            }
        }
        "remove" => {
            let Some(e) = app.apps.borrow().iter().find(|a| a.str("id") == Some(id)).cloned() else { return };
            let name = e.str("name").unwrap_or("");
            // removeItem's messages.
            let msg = if e.str("kind") == Some("island") {
                format!("Remove island \"{name}\"? Its contents will move up one level.")
            } else if e.str("type") == Some("stored") {
                let at = e.str("originalPath").filter(|p| !p.is_empty()).map(|p| format!(" at {p}")).unwrap_or_default();
                format!("Remove \"{name}\"?\n\nLaunchpad's stored copy will be deleted permanently, not moved to the Recycle Bin. The original{at} is not touched.")
            } else {
                format!("Remove \"{name}\" from Launchpad? (This does not delete the app itself.)")
            };
            let id = id.to_string();
            modals::confirm(app, &msg, "OK", false, move |app| modals::remove_item(app, &id));
        }
        _ => {}
    }
}

// ---- Open windows --------------------------------------------------------

fn push_windows(_app: &App, w: &MainWindow) {
    let tiles: Vec<TaskbarWindow> = WINDOWS.with(|s| {
        let s = s.borrow();
        s.list
            .iter()
            .map(|(_, title, exe)| {
                let icon = exe.as_ref().and_then(|e| s.icons.get(e).cloned().flatten());
                TaskbarWindow {
                    title: title.as_str().into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                    letter: title.trim().chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or("?".into()).into(),
                }
            })
            .collect()
    });
    w.global::<TaskbarState>().set_windows(model(tiles));
}

/// Reads the window list (and any new exe's icon) on a worker thread. One
/// read at a time; a request during one runs once it's back.
fn refresh_windows(app: &Rc<App>) {
    let Some(w) = app.window.borrow().as_ref().map(|w| w.clone_strong()) else { return };
    let known: Option<Vec<String>> = WINDOWS.with(|s| {
        let mut s = s.borrow_mut();
        if s.busy {
            s.again = true;
            return None;
        }
        s.busy = true;
        Some(s.icons.keys().cloned().collect())
    });
    let Some(known) = known else { return };
    let size = px(&w, 26.0);
    std::thread::spawn(move || {
        let list: Vec<(i64, String, Option<String>)> = windowmgr::taskbar_windows().into_iter().map(|w| (w.hwnd, w.title, w.exe)).collect();
        let mut fresh: Vec<(String, Option<crate::win::image::Rgba>)> = Vec::new();
        let wic = Wic::new().ok();
        for exe in list.iter().filter_map(|w| w.2.as_ref()) {
            if !known.contains(exe) && !fresh.iter().any(|(e, _)| e == exe) {
                fresh.push((exe.clone(), wic.as_ref().and_then(|wic| wic.file_icon(Path::new(exe), size).ok())));
            }
        }
        post(move |app| {
            let (changed, again) = WINDOWS.with(|s| {
                let mut s = s.borrow_mut();
                // Unchanged (a title change elsewhere): keep the tiles, and their hover.
                let changed = !fresh.is_empty() || s.list != list;
                for (exe, rgba) in fresh {
                    s.icons.insert(exe, rgba.map(super::images::to_image));
                }
                s.list = list;
                s.busy = false;
                (changed, std::mem::take(&mut s.again))
            });
            if let Some(w) = app.window.borrow().as_ref().filter(|_| changed) {
                push_windows(app, w);
            }
            if again {
                refresh_windows(app);
            }
        });
    });
}
