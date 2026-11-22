//! See ui/settings.slint. Ported from renderer.js "settings modal",
//! "Settings sections and search", the hotkey capture, "reset (Settings →
//! Danger zone)", and main.js's settings:*, update:*, app:reinstall,
//! backup:*, settings:reset and apps:detectMissingIcons/restoreDefaults/
//! repairDefaults handlers. Each control reads and writes what 1.2.2 did:
//! settings.json keys, or ui.json prefs under 1.2.2's localStorage keys.
use super::modals::{alert, confirm, js_trim};
use super::{icons, App, MainWindow, Popouts, SettingsState, DEFAULT_HOTKEY};
use crate::win::{autostart, dialogs, hotkey, launch, services};
use launchpad_core::media::library::Library;
use launchpad_core::store::{self, Store};
use launchpad_core::{backup, items, reset, settings as core_settings, time, truthy, update};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES_URL: &str = "https://github.com/Jurakii/Launchpad/releases";
const SLINT_URL: &str = "https://slint.dev";
const SECTION_PREF: &str = "launchpad-settings-section";

/// (id, title, blurb), in the sidebar's order.
const SECTIONS: [(&str, &str, &str); 8] = [
    ("general", "General", "How Launchpad starts and what the home grid shows."),
    ("appearance", "Appearance", "Theme, accent color, wallpaper and the look of the top bar."),
    ("taskbar", "Taskbar", "Where the taskbar sits and how its buttons line up."),
    ("hotkeys", "Hotkeys", "Keyboard shortcuts that work from anywhere in Windows."),
    ("organizer", "Window organizer", "A panel along the top of each screen for snapping windows into layouts."),
    ("privacy", "Icons & privacy", "Where app and website icons come from."),
    ("backup", "Backup & updates", "Keep a copy of your setup and stay current."),
    ("danger", "Danger zone", "Start over from a fresh install."),
];

/// Every setting: (section, label, hint, the rest of its text that search
/// matches: button labels and options, as 1.2's `textContent` held them).
/// The index is the item's number in settings.slint.
const ITEMS: [(usize, &str, &str, &str); 34] = [
    (0, "Start with Windows", "", ""),
    (0, "Start fullscreen", "", ""),
    (0, "Minimize to tray on close", "Closing the window keeps Launchpad in the system tray. Use the tray icon's Quit to exit.", ""),
    (0, "Show \"New Island\" and \"Add App\" buttons", "", ""),
    (0, "Show app names", "", ""),
    (0, "Copy dropped files into Launchpad", "Dropped files and folders are kept as copies, so they still open if the original moves. Programs and shortcuts are always added as links. Hold Alt while dropping to add a link instead.", ""),
    (1, "Light theme", "", ""),
    (1, "Accent color", "Used for focus rings, selections and active buttons.", "Reset"),
    (1, "Wallpaper", "", "Choose Image… Clear"),
    (1, "Top bar look", "", "Solid Translucent Transparent"),
    (2, "Look", "", "Solid Translucent Transparent"),
    (2, "Location", "", "Bottom Top Left Right"),
    (2, "Alignment", "", "Edge (Windows 10) Center (Windows 11)"),
    (2, "Buttons", "", "Left / Top Right / Bottom"),
    (2, "System tray position", "Drag the clock, volume and network icons to reorder them. Right-click an empty spot on the taskbar to change these quickly.", "Snap to edge Snap to center Same side as taskbar buttons"),
    (3, "Show or hide Launchpad", "Click the button, then press the new keys.", ""),
    (3, "Disable hotkey while another app is focused", "The keys go to games and editors instead. The hotkey still works from the desktop and taskbar.", ""),
    (3, "Grid placer", "Opens a grid over the screen to size the window you're using: drag across cells, or use the arrow keys (Shift+arrows to resize) and Enter. Needs the window organizer.", ""),
    (4, "Window organizer", "Click a zone in one of the panel's layouts to move the window you're using there. Right-click an open window on the taskbar for the same layouts. While the panel is on, it also carries Launchpad's top bar on the screen Launchpad is on, and follows the top bar look. Make your own with Layouts.", ""),
    (4, "Organizer panel", "Auto-hide brings the panel down when the pointer rests against the top edge. Fullscreen apps cover it, apart from Launchpad itself.", "Always shown Auto-hide Off"),
    (4, "Snap windows by dragging them to the top of the screen", "Drop the window on a zone in the layouts that appear. Press Esc or move away to cancel.", ""),
    (5, "Use Google to find website icons as a fallback", "Only when a site has no icon of its own. Google then sees the site's address.", ""),
    (5, "Missing icons", "", "Detect Missing Icons"),
    (5, "Default apps", "Restore adds File Explorer or the Recycle Bin if they're gone. Repair also puts back what they open and their icons, as this version ships them. Names, islands and positions are kept.", "Restore Default Apps Repair Default Apps"),
    (6, "Backup", "Includes your apps, layouts, and the Media library and players. Artwork isn't included.", "Export Config… Import Config…"),
    (6, "Notify me when an update is available", "", ""),
    (6, "", "", "Launchpad version update Made with Slint"),
    (6, "Releases", "", "Open GitHub Releases"),
    (6, "Repair Launchpad", "Downloads this version's installer from its GitHub release and runs it, like an update. Your apps and settings are kept. Launchpad restarts when it's done.", "Reinstall This Version…"),
    (7, "Reset Launchpad", "Removes your apps, islands, icons, stored files, layouts, scenes and settings, and restarts Launchpad as a first install. Your Media library, players and artwork are kept.", "Reset Launchpad…"),
    // Appended so the indices above (and the .slint's) stay put; shown under Appearance.
    (1, "Interface size", "Makes all of Launchpad's text and controls bigger. Launchpad restarts to apply it.", "100% 110% 125% 150% 175%"),
    (5, "SteamGridDB API key", "Adds game icons to Search Icons… and lets Media find covers, banners and logos (Customize art → Find Art Online). Free: sign in at steamgriddb.com, then Preferences → API.", ""),
    (5, "ScreenScraper account", "Lets Media find box art, fanart and logos for console and PC games on screenscraper.fr (Customize art → Find Art Online). Your free screenscraper.fr account; it raises your daily limit.", "user name password"),
    (5, "ScreenScraper developer login", "Leave empty to use the one Launchpad was built with. A developer id comes from ScreenScraper's forum.", "developer id password"),
];

/// Settings → Appearance → Interface size, as settings.json's `uiScale`.
pub const UI_SCALES: [f64; 5] = [1.0, 1.1, 1.25, 1.5, 1.75];

const BAR_STYLES: [&str; 3] = ["solid", "translucent", "transparent"];
const EDGES: [&str; 4] = ["bottom", "top", "left", "right"];
const ALIGNS: [&str; 2] = ["edge", "center"];
const SIDES: [&str; 2] = ["left", "right"];
const TRAY_ALIGNS: [&str; 3] = ["edge", "center", "shared"];

#[derive(Clone, Default, PartialEq)]
enum Update {
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available(String),
    Downloading(String),
    Error(String),
}

#[derive(Default)]
struct Reset {
    backed_up: bool,
    backup_busy: bool,
    status: String,
    no_backup: bool,
    text: String,
    busy: bool,
}

impl Reset {
    fn can_confirm(&self) -> bool {
        !self.busy && (self.backed_up || self.no_backup) && js_trim(&self.text) == "RESET"
    }
}

#[derive(Default)]
struct State {
    open: bool,
    /// None until the remembered one is read from ui.json.
    section: Option<usize>,
    query: String,
    capturing: i32,
    update: Update,
    /// Detect, Restore, Repair, Reinstall.
    busy: [bool; 4],
    /// Test login is signing in to ScreenScraper.
    ss_testing: bool,
    reset: Option<Reset>,
    accent_hex: Option<String>,
    focus_tick: i32,
    scroll_tick: i32,
    started: bool,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State { capturing: -1, ..Default::default() });
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    ST.with(|s| f(&mut s.borrow_mut()))
}

fn ss(v: &str) -> SharedString {
    v.into()
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

// ---------- search ----------

struct View {
    hits: Vec<bool>,
    firsts: Vec<bool>,
    shown: Vec<bool>,
    counts: Vec<usize>,
    total: usize,
}

/// renderer.js `renderSettingsSections`: without a query, the current
/// section; with one, every item whose section title and text contain it.
fn view(section: usize, query: &str) -> View {
    let q = js_trim(query).to_lowercase();
    let mut v = View { hits: vec![false; ITEMS.len()], firsts: vec![false; ITEMS.len()], shown: vec![false; 8], counts: vec![0; 8], total: 0 };
    for (i, (sec, label, hint, extra)) in ITEMS.iter().enumerate() {
        let hit = if q.is_empty() { *sec == section } else { format!("{} {label} {hint} {extra}", SECTIONS[*sec].1).to_lowercase().contains(&q) };
        if hit {
            v.firsts[i] = v.counts[*sec] == 0;
            v.counts[*sec] += 1;
            v.total += 1;
        }
        v.hits[i] = hit;
    }
    for k in 0..8 {
        v.shown[k] = if q.is_empty() { k == section } else { v.counts[k] > 0 };
    }
    v
}

/// formatAccelerator.
fn format_accel(a: &str) -> String {
    a.replacen("CommandOrControl", "Ctrl", 1).replacen("Command", "Cmd", 1)
}

/// acceleratorFromEvent, from Slint's key text. None for a modifier alone
/// (keep listening).
fn accel_from_key(text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<String> {
    use slint::platform::Key;
    let mut chars = text.chars();
    let c = chars.next()?;
    let single = chars.next().is_none();
    let is = |k: Key| single && c == char::from(k);
    if [Key::Control, Key::ControlR, Key::Alt, Key::AltGr, Key::Shift, Key::ShiftR, Key::Meta, Key::MetaR].into_iter().any(is) {
        return None;
    }
    let named: &[(Key, &str)] = &[
        (Key::UpArrow, "Up"),
        (Key::DownArrow, "Down"),
        (Key::LeftArrow, "Left"),
        (Key::RightArrow, "Right"),
        (Key::Return, "Enter"),
        (Key::Tab, "Tab"),
        (Key::Backspace, "Backspace"),
        (Key::Delete, "Delete"),
        (Key::Insert, "Insert"),
        (Key::Home, "Home"),
        (Key::End, "End"),
        (Key::PageUp, "PageUp"),
        (Key::PageDown, "PageDown"),
        (Key::Escape, "Escape"),
        (Key::Pause, "Pause"),
        (Key::ScrollLock, "ScrollLock"),
        (Key::SysReq, "PrintScreen"),
        (Key::Menu, "ContextMenu"),
    ];
    let fkeys = [
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12,
        Key::F13, Key::F14, Key::F15, Key::F16, Key::F17, Key::F18, Key::F19, Key::F20, Key::F21, Key::F22, Key::F23, Key::F24,
    ];
    let main = if let Some((_, n)) = named.iter().find(|(k, _)| is(*k)) {
        n.to_string()
    } else if let Some(n) = fkeys.iter().position(|k| is(*k)) {
        format!("F{}", n + 1)
    } else if c == ' ' && single {
        "Space".into()
    } else if (c as u32) < 0x20 && single {
        // Ctrl+letter can come through as a control character.
        char::from_u32(c as u32 + 0x40)?.to_string()
    } else {
        text.to_uppercase()
    };
    let mut parts = Vec::new();
    if ctrl || meta {
        parts.push("CommandOrControl".to_string());
    }
    if alt {
        parts.push("Alt".into());
    }
    if shift {
        parts.push("Shift".into());
    }
    parts.push(main);
    Some(parts.join("+"))
}

// ---------- pushing state ----------

fn default_accent(light: bool) -> &'static str {
    if light {
        "#3b6fe0"
    } else {
        "#5b8cff"
    }
}

fn push(app: &App, w: &MainWindow) {
    let g = w.global::<SettingsState>();
    let open = st(|s| s.open);
    g.set_open(open);
    if !open {
        return;
    }
    let section = current_section(app);
    let s = app.settings();
    let light = app.pref("launchpad-theme").as_deref() == Some("light");
    let accent = app.pref("launchpad-accent").unwrap_or_else(|| default_accent(light).into());
    let pick = |key: &str, opts: &[&str]| -> i32 { app.pref(key).and_then(|v| opts.iter().position(|o| *o == v)).unwrap_or(0) as i32 };
    let org = core_settings::organizer_options(&s);
    let hotkey = s.get("hotkey").and_then(Value::as_str).filter(|h| !h.is_empty()).unwrap_or(DEFAULT_HOTKEY).to_string();

    g.set_autostart(autostart::is_enabled());
    g.set_fullscreen(truthy(s.get("startFullscreen")));
    g.set_minimize_to_tray(s.get("minimizeToTrayOnClose") != Some(&Value::Bool(false)));
    g.set_show_add_buttons(app.pref("launchpad-hide-add-buttons").as_deref() != Some("1"));
    g.set_show_app_names(app.pref("launchpad-hide-app-names").as_deref() != Some("1"));
    g.set_file_store_copy(s.get("fileStoreOnDrop").and_then(Value::as_str) != Some("link"));
    g.set_light(light);
    g.set_accent(super::prefs::parse_hex(&accent).unwrap_or_default());
    g.set_topbar_style(pick("launchpad-topbar-style", &BAR_STYLES));
    g.set_ui_scale(UI_SCALES.iter().position(|v| Some(*v) == s.get("uiScale").and_then(Value::as_f64)).unwrap_or(0) as i32);
    g.set_taskbar_style(pick("launchpad-taskbar-style", &BAR_STYLES));
    g.set_taskbar_edge(pick("launchpad-taskbar-edge", &EDGES));
    g.set_taskbar_align(pick("launchpad-taskbar-align", &ALIGNS));
    g.set_taskbar_position(pick("launchpad-taskbar-position", &SIDES));
    g.set_tray_align(pick("launchpad-tray-align", &TRAY_ALIGNS));
    g.set_hotkey_label(ss(&format_accel(&hotkey)));
    g.set_grid_hotkey_label(ss(&format_accel(&org.grid_hotkey)));
    g.set_disable_hotkey_in_apps(truthy(s.get("disableHotkeyInApps")));
    g.set_organizer(org.enabled);
    g.set_organizer_panel(core_settings::PANEL_MODES.iter().position(|p| *p == org.panel).unwrap_or(0) as i32);
    g.set_snap_on_drag(org.snap_on_drag);
    g.set_google_favicon(icons::google_fallback(app));
    g.set_sgdb_key(s.get("steamGridDbKey").and_then(Value::as_str).unwrap_or("").into());
    let text = |k: &str| ss(s.get(k).and_then(Value::as_str).unwrap_or(""));
    g.set_ss_user(text("screenScraperUser"));
    g.set_ss_password(text("screenScraperPassword"));
    g.set_ss_dev_id(text("screenScraperDevId"));
    g.set_ss_dev_password(text("screenScraperDevPassword"));
    g.set_notify_updates(truthy(s.get("notifyUpdates")));
    g.set_version_label(ss(&format!("Launchpad v{VERSION}")));

    st(|st| {
        let v = view(section, &st.query);
        let searching = !js_trim(&st.query).is_empty();
        g.set_section(section as i32);
        g.set_query(ss(&st.query));
        g.set_searching(searching);
        g.set_hits(model(v.hits));
        g.set_firsts(model(v.firsts));
        g.set_section_shown(model(v.shown));
        g.set_nav_counts(model(v.counts.iter().map(|&c| if searching && c > 0 { ss(&c.to_string()) } else { ss("") }).collect()));
        g.set_nav_no_match(model(v.counts.iter().map(|&c| searching && c == 0).collect()));
        if searching {
            g.set_heading(ss(&format!("Results for \"{}\"", js_trim(&st.query))));
            g.set_blurb(ss(&if v.total == 1 { "1 setting".to_string() } else { format!("{} settings", v.total) }));
        } else {
            g.set_heading(ss(SECTIONS[section].1));
            g.set_blurb(ss(SECTIONS[section].2));
        }
        g.set_no_results(searching && v.total == 0);
        g.set_accent_hex(ss(st.accent_hex.as_deref().unwrap_or(&accent)));
        g.set_capturing(st.capturing);
        let [d, r, p, ri] = st.busy;
        g.set_busy_detect(d);
        g.set_busy_restore(r);
        g.set_busy_repair(p);
        g.set_busy_reinstall(ri);
        g.set_detect_label(ss(if d { "Scanning…" } else { "Detect Missing Icons" }));
        g.set_ss_testing(st.ss_testing);
        g.set_restore_label(ss(if r { "Adding…" } else { "Restore Default Apps" }));
        g.set_repair_label(ss(if p { "Repairing…" } else { "Repair Default Apps" }));
        g.set_reinstall_label(ss(if ri { "Downloading…" } else { "Reinstall This Version…" }));
        let (status, action) = match &st.update {
            Update::Unknown => (String::new(), ""),
            Update::Checking => ("Checking for updates…".into(), ""),
            Update::UpToDate => ("You're up to date.".into(), ""),
            Update::Available(v) => (format!("Update available: v{v}"), if portable() { "Open Download Page" } else { "Download & Install" }),
            Update::Downloading(v) => (format!("Downloading v{v}…"), ""),
            Update::Error(m) => (format!("Update check failed: {m}"), ""),
        };
        g.set_update_status(ss(&status));
        g.set_update_action(ss(action));
        g.set_update_busy(false);
        match &st.reset {
            Some(r) => {
                g.set_reset_backup_status(ss(&r.status));
                g.set_reset_backup_busy(r.backup_busy);
                g.set_reset_no_backup(r.no_backup);
                g.set_reset_text(ss(&r.text));
                g.set_reset_can_confirm(r.can_confirm());
                g.set_reset_confirm_label(ss(if r.busy { "Resetting…" } else { "Reset and Restart" }));
                g.set_reset_open(true);
            }
            None => g.set_reset_open(false),
        }
        g.set_focus_tick(st.focus_tick);
        g.set_scroll_tick(st.scroll_tick);
    });
}

fn redraw(app: &App) {
    if let Some(w) = app.window.borrow().as_ref() {
        push(app, w);
    }
}

fn current_section(app: &App) -> usize {
    if let Some(s) = st(|s| s.section) {
        return s;
    }
    let remembered = app.pref(SECTION_PREF).and_then(|id| SECTIONS.iter().position(|s| s.0 == id)).unwrap_or(0);
    st(|s| s.section = Some(remembered));
    remembered
}

/// showSettingsSection: remembered in ui.json, search cleared, scrolled to the top.
fn show_section(app: &App, k: usize) {
    let k = k.min(SECTIONS.len() - 1);
    app.set_pref(SECTION_PREF, Some(SECTIONS[k].0));
    st(|s| {
        s.section = Some(k);
        s.query.clear();
        s.scroll_tick += 1;
    });
    redraw(app);
}

/// Puts the keyboard focus back on Settings (after a dialog over it closed).
pub fn refocus(app: &App) {
    if st(|s| s.open) {
        st(|s| s.focus_tick += 1);
        redraw(app);
    }
}

/// Wires the global's callbacks on a newly made window.
pub fn bind(app: &Rc<App>, w: &MainWindow) {
    let g = w.global::<SettingsState>();
    g.set_nav_names(model(SECTIONS.iter().map(|s| ss(s.1)).collect()));
    g.set_labels(model(ITEMS.iter().map(|i| ss(i.1)).collect()));
    g.set_hints(model(ITEMS.iter().map(|i| ss(i.2)).collect()));

    g.on_search(|q| {
        super::with(|app| {
            st(|s| s.query = q.to_string());
            redraw(app);
        });
    });
    g.on_pick_section(|k| {
        super::with(|app| show_section(app, k as usize));
    });
    g.on_close(|| {
        super::with(|app| close(app));
    });
    g.on_toggle(|key, v| {
        super::with(|app| toggle(app, &key, v));
    });
    let weak = w.as_weak();
    g.on_select(move |key, i| {
        super::with(|app| select(app, &key, i.max(0) as usize));
        if key == "taskbar-edge" {
            if let Some(w) = weak.upgrade() {
                w.global::<Popouts>().set_open(ss(""));
            }
        }
    });
    g.on_sgdb_edited(|t| {
        super::with(|app| save(app, json!({ "steamGridDbKey": t.trim() })));
    });
    g.on_text_edited(|key, t| {
        super::with(|app| save(app, json!({ key.as_str(): t.trim() })));
    });
    g.on_accent_edited(|t| {
        super::with(|app| {
            st(|s| s.accent_hex = Some(t.to_string()));
            if let Some(hex) = super::modals::normalize_hex(&t) {
                app.set_pref("launchpad-accent", Some(&hex));
                redraw(app);
            }
        });
    });
    g.on_hotkey_key(|text, ctrl, alt, shift, meta| {
        super::with(|app| hotkey_key(app, &text, ctrl, alt, shift, meta));
    });
    g.on_reset_edited(|t| {
        super::with(|app| {
            st(|s| s.reset.iter_mut().for_each(|r| r.text = t.to_string()));
            redraw(app);
        });
    });
    g.on_action(|key| {
        let key = key.to_string();
        // Some open dialogs: start everything from a timer, not from inside Slint's dispatch.
        super::later(move |app| action(app, &key));
    });

    push(app, w);

    // Once per run, 1.2's background check for the opt-in notification.
    if !st(|s| std::mem::replace(&mut s.started, true)) && truthy(app.settings().get("notifyUpdates")) {
        check_for_updates(app, true);
    }
}

/// Pushes the current state into the window.
pub fn refresh(app: &App, w: &MainWindow) {
    push(app, w);
}

/// Opens Settings, at `section` ("general", "appearance", "taskbar",
/// "hotkeys", "organizer", "privacy", "backup", "danger") or the last one shown.
pub fn open(app: &Rc<App>, section: Option<&str>) {
    let k = section.and_then(|id| SECTIONS.iter().position(|s| s.0 == id)).unwrap_or_else(|| current_section(app));
    st(|s| {
        s.open = true;
        s.accent_hex = None;
        s.focus_tick += 1;
    });
    show_section(app, k);
    // A download in progress isn't interrupted by a fresh check.
    if !matches!(st(|s| s.update.clone()), Update::Downloading(_)) {
        check_for_updates(app, false);
    }
}

fn close(app: &App) {
    st(|s| {
        s.open = false;
        s.capturing = -1;
        s.reset = None;
    });
    redraw(app);
}

// ---------- controls ----------

fn save(app: &Rc<App>, patch: Value) {
    if let Err(e) = app.save_settings(patch) {
        alert(app, &format!("Couldn't save the setting:\n{e}"));
    }
}

fn toggle(app: &Rc<App>, key: &str, v: bool) {
    match key {
        "autostart" => {
            if let Err(e) = autostart::set_enabled(v) {
                alert(app, &format!("Start with Windows: {e}"));
            }
        }
        "startFullscreen" | "minimizeToTrayOnClose" | "notifyUpdates" | "googleFaviconFallback" => save(app, json!({ key: v })),
        "organizerEnabled" | "organizerSnapOnDrag" => {
            save(app, json!({ key: v }));
            super::organizer::configure(app);
        }
        "disableHotkeyInApps" => {
            save(app, json!({ "disableHotkeyInApps": v }));
            app.set_hotkey_follows_focus(v);
        }
        "fileStoreOnDrop" => save(app, json!({ "fileStoreOnDrop": if v { "copy" } else { "link" } })),
        "show-add-buttons" => app.set_pref("launchpad-hide-add-buttons", (!v).then_some("1")),
        "show-app-names" => app.set_pref("launchpad-hide-app-names", (!v).then_some("1")),
        "light" => {
            app.set_pref("launchpad-theme", Some(if v { "light" } else { "dark" }));
            st(|s| s.accent_hex = None);
        }
        "reset-no-backup" => st(|s| s.reset.iter_mut().for_each(|r| r.no_backup = v)),
        _ => {}
    }
    redraw(app);
}

fn s_scale(app: &Rc<App>) -> f64 {
    app.settings().get("uiScale").and_then(Value::as_f64).unwrap_or(1.0)
}

/// The selects. Defaults are removed rather than written, as 1.2 did.
fn select(app: &Rc<App>, key: &str, i: usize) {
    let set = |pref: &str, opts: &[&str]| {
        let v = opts.get(i).copied().unwrap_or(opts[0]);
        app.set_pref(pref, (v != opts[0]).then_some(v));
    };
    match key {
        "topbar-style" => set("launchpad-topbar-style", &BAR_STYLES),
        "taskbar-style" => set("launchpad-taskbar-style", &BAR_STYLES),
        "taskbar-edge" => set("launchpad-taskbar-edge", &EDGES),
        "taskbar-align" => set("launchpad-taskbar-align", &ALIGNS),
        "taskbar-position" => set("launchpad-taskbar-position", &SIDES),
        "tray-align" => set("launchpad-tray-align", &TRAY_ALIGNS),
        "ui-scale" => {
            let v = UI_SCALES.get(i).copied().unwrap_or(1.0);
            if s_scale(app) != v {
                save(app, json!({ "uiScale": v }));
                app.relaunch();
            }
            return;
        }
        "organizerPanel" => {
            if let Some(mode) = core_settings::PANEL_MODES.get(i) {
                save(app, json!({ "organizerPanel": mode }));
                super::organizer::configure(app);
            }
        }
        _ => {}
    }
    // The taskbar's look comes from the Theme global; it needs its own refresh too.
    app.refresh();
}

fn hotkey_key(app: &Rc<App>, text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) {
    let which = st(|s| s.capturing);
    if which < 0 {
        return;
    }
    if text == char::from(slint::platform::Key::Escape).to_string() && !ctrl && !alt && !shift && !meta {
        st(|s| s.capturing = -1);
        return redraw(app);
    }
    let Some(accel) = accel_from_key(text, ctrl, alt, shift, meta) else { return };
    st(|s| s.capturing = -1);
    let s = app.settings();
    let result = if which == 0 {
        set_main_hotkey(app, &s, &accel)
    } else {
        set_grid_hotkey(app, &s, &accel)
    };
    if let Err(msg) = result {
        alert(app, &msg);
    }
    redraw(app);
}

/// settings:setHotkey: the old keys stay registered if the new ones are taken.
fn set_main_hotkey(app: &Rc<App>, s: &core_settings::Settings, accel: &str) -> Result<(), String> {
    let org = core_settings::organizer_options(s);
    if org.enabled && hotkey::same(accel, &org.grid_hotkey) {
        return Err("That's already the grid placer's hotkey.".into());
    }
    if !app.hotkey.borrow_mut().set(accel) {
        return Err(format!("\"{accel}\" is already in use by another app."));
    }
    app.save_settings(json!({ "hotkey": accel })).map(|_| ()).map_err(|e| e.to_string())
}

/// settings:setOrganizerHotkey: the old keys stay registered if the new ones are taken.
fn set_grid_hotkey(app: &Rc<App>, s: &core_settings::Settings, accel: &str) -> Result<(), String> {
    let main = s.get("hotkey").and_then(Value::as_str).filter(|h| !h.is_empty()).unwrap_or(DEFAULT_HOTKEY);
    if hotkey::same(accel, main) {
        return Err("That's already Launchpad's show/hide hotkey.".into());
    }
    if hotkey::parse(accel).is_none() {
        return Err("Couldn't set that hotkey.".into());
    }
    super::organizer::set_grid_hotkey(app, accel)?;
    app.save_settings(json!({ "organizerHotkey": accel })).map(|_| ()).map_err(|e| e.to_string())
}

fn action(app: &Rc<App>, key: &str) {
    match key {
        "accent-reset" => {
            app.set_pref("launchpad-accent", None);
            st(|s| s.accent_hex = None);
        }
        "wallpaper-pick" => pick_wallpaper(app),
        "wallpaper-clear" => clear_wallpaper(app),
        "capture-main" | "capture-grid" => st(|s| {
            if s.capturing < 0 {
                s.capturing = if key == "capture-main" { 0 } else { 1 };
            }
        }),
        "detect-icons" => detect_icons(app),
        "ss-test" => ss_test(app),
        "restore-defaults" => defaults(app, false),
        "repair-defaults" => defaults(app, true),
        "export" => export(app, false),
        "import" => confirm(app, "Importing will replace everything currently in Launchpad with this backup. Continue?", "OK", false, |_| {
            super::later(import)
        }),
        "update" => install_update(app),
        "releases" => open_url(RELEASES_URL),
        "open-slint" => open_url(SLINT_URL),
        "reinstall" => confirm(
            app,
            &format!("Download Launchpad {VERSION} from GitHub and reinstall it? Launchpad closes while it installs and starts again afterwards. Your apps and settings are kept."),
            "OK",
            false,
            reinstall,
        ),
        "reset-open" => st(|s| s.reset = Some(Reset::default())),
        "reset-cancel" => {
            st(|s| s.reset = None);
            super::later(|app| refocus(app));
        }
        "reset-backup" => export(app, true),
        "reset-confirm" => run_reset(app),
        _ => {}
    }
    redraw(app);
}

fn open_url(url: &'static str) {
    std::thread::spawn(move || {
        if let Err(e) = launch::open_external(url) {
            eprintln!("Opening {url}: {e}");
        }
    });
}

/// Copies the picked image into the data folder as wallpaper.<ext> and
/// names it in `launchpad-wallpaper` (core::ui_prefs).
fn pick_wallpaper(app: &Rc<App>) {
    let filters: [dialogs::Filter; 1] = [("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])];
    let src = match dialogs::open_file(app.hwnd(), &filters, None) {
        Ok(Some(p)) => p,
        Ok(None) => return,
        Err(e) => return alert(app, &e.to_string()),
    };
    set_wallpaper(app, src);
}

fn set_wallpaper(app: &Rc<App>, src: PathBuf) {
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or_else(|| "png".into());
    let name = format!("wallpaper.{ext}");
    let old = app.pref(ui_wallpaper_key());
    let dir = app.data_dir.clone();
    std::thread::spawn(move || {
        let dest = dir.join(&name);
        let res = std::fs::copy(&src, &dest);
        super::post(move |app| match res {
            Ok(_) => {
                if let Some(old) = old.filter(|o| *o != name && is_wallpaper_name(o)) {
                    let _ = std::fs::remove_file(app.data_dir.join(old));
                }
                app.images.forget(&dest);
                app.set_pref(ui_wallpaper_key(), Some(&name));
            }
            Err(e) => alert(app, &format!("Couldn't set that wallpaper:\n{e}")),
        });
    });
}

fn ui_wallpaper_key() -> &'static str {
    launchpad_core::ui_prefs::WALLPAPER_KEY
}

/// Only ever delete a file this module wrote.
fn is_wallpaper_name(name: &str) -> bool {
    name.strip_prefix("wallpaper.").is_some_and(|ext| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn clear_wallpaper(app: &Rc<App>) {
    if let Some(name) = app.pref(ui_wallpaper_key()).filter(|n| is_wallpaper_name(n)) {
        let path = app.data_dir.join(name);
        app.images.forget(&path);
        let _ = std::fs::remove_file(path);
    }
    app.set_pref(ui_wallpaper_key(), None);
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn detect_icons(app: &Rc<App>) {
    st(|s| s.busy[0] = true);
    let tasks = items::detect_missing_icons(&app.apps.borrow(), |p| Path::new(p).exists());
    icons::spawn(app, tasks, |fixed| {
        super::with(|app| {
            st(|s| s.busy[0] = false);
            redraw(app);
            alert(app, &if fixed > 0 { format!("Fixed {}.", plural(fixed, "icon")) } else { "No missing icons found.".into() });
        });
    });
}

/// Test login: signs in to ScreenScraper with what's typed (saved as it's
/// typed) and says whether it worked.
fn ss_test(app: &Rc<App>) {
    let login = super::media::forms::ss_login(app).unwrap_or_default();
    st(|s| s.ss_testing = true);
    redraw(app);
    std::thread::spawn(move || {
        let result = launchpad_core::art_search::ss_check(&login, &mut services::Search);
        super::post(move |app| {
            st(|s| s.ss_testing = false);
            redraw(app);
            alert(app, &result.unwrap_or_else(|e| e));
        });
    });
}

fn defaults(app: &Rc<App>, repair: bool) {
    let slot = if repair { 2 } else { 1 };
    let res = app.mutate(|apps| {
        let now = time::now_ms();
        Ok(if repair { items::repair_defaults(apps, store::new_id, now) } else { items::restore_defaults(apps, store::new_id, now) })
    });
    let change = match res {
        Ok(c) => c,
        Err(e) => {
            let what = if repair { "repair" } else { "add" };
            return alert(app, &format!("Couldn't {what} the default apps:\n{e}"));
        }
    };
    st(|s| s.busy[slot] = true);
    icons::spawn(app, change.icons, move |_| {
        super::with(|app| {
            st(|s| s.busy[slot] = false);
            redraw(app);
            let msg = if repair {
                let mut parts = Vec::new();
                if change.repaired > 0 {
                    parts.push(format!("repaired {}", change.repaired));
                }
                if change.added > 0 {
                    parts.push(format!("added {}", change.added));
                }
                if parts.is_empty() {
                    "Nothing to repair.".into()
                } else {
                    format!("Default apps: {}.", parts.join(", "))
                }
            } else if change.added > 0 {
                format!("Added {}.", plural(change.added, "default app"))
            } else {
                "Already on your list.".into()
            };
            alert(app, &msg);
        });
    });
}

// ---------- backup ----------

const BACKUP_FILTER: [dialogs::Filter; 1] = [("Launchpad Backup", &["json"])];

/// backup:export; for the reset dialog's "Back Up First…" the result goes
/// into its status line instead of an alert.
fn export(app: &Rc<App>, for_reset: bool) {
    let name = format!("launchpad-backup-{}.json", &time::now_iso()[..10]);
    let file = match dialogs::save_file(app.hwnd(), &BACKUP_FILTER, Some(&name), None) {
        Ok(Some(f)) => f,
        Ok(None) => return,
        Err(e) => return alert(app, &e.to_string()),
    };
    export_to(app, file, for_reset);
}

fn export_to(app: &Rc<App>, file: PathBuf, for_reset: bool) {
    if for_reset {
        st(|s| s.reset.iter_mut().for_each(|r| r.backup_busy = true));
        redraw(app);
    }
    let dir = app.data_dir.clone();
    std::thread::spawn(move || {
        let res = backup::export(&mut Store::new(&dir), &mut Library::new(&dir), &file);
        super::post(move |app| {
            if for_reset {
                st(|s| {
                    if let Some(r) = s.reset.as_mut() {
                        r.backup_busy = false;
                        match &res {
                            Ok(x) => {
                                r.backed_up = true;
                                r.status = format!("Saved to {}", x.path.display());
                            }
                            Err(e) => r.status = format!("Backup failed: {e}"),
                        }
                    }
                });
                return redraw(app);
            }
            match res {
                Ok(x) => {
                    let mut msg = format!("Exported to:\n{}", x.path.display());
                    if let Some(d) = &x.stored_files_dir {
                        msg += &format!("\n\nStored files were copied to:\n{}\nKeep that folder next to the backup file.", d.display());
                    }
                    if !x.stored_missing.is_empty() {
                        msg += &format!("\n\nThese stored files were missing, so they aren't in the backup:\n{}", bullets(&x.stored_missing));
                    }
                    alert(app, &msg);
                }
                Err(e) => alert(app, &format!("Couldn't export a backup:\n{e}")),
            }
        });
    });
}

fn bullets(names: &[String]) -> String {
    names.iter().map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n")
}

fn import(app: &Rc<App>) {
    let file = match dialogs::open_file(app.hwnd(), &BACKUP_FILTER, None) {
        Ok(Some(f)) => f,
        Ok(None) => return,
        Err(e) => return alert(app, &e.to_string()),
    };
    import_from(app, file);
}

fn import_from(app: &Rc<App>, file: PathBuf) {
    // On the UI thread: it replaces apps.json, and an icon or edit saved from
    // the old list between its write and taking its result would undo it.
    // shortcut: stored files are copied here too, blocking; move that copy to
    // a worker (with the new list swapped in under a guard) if it's ever slow.
    let res = backup::import(&mut Store::new(&app.data_dir), &mut Library::new(&app.data_dir), &file);
    let x = match res {
        Ok(x) => x,
        Err(e) => return alert(app, &format!("Couldn't import that backup:
{e}")),
    };
    // apps.json is good again: a fresh store, not read-only any more.
    *app.store.borrow_mut() = Store::new(&app.data_dir);
    *app.apps.borrow_mut() = x.apps;
    app.images.clear();
    close(app);
    app.refresh();
    super::organizer::layouts_changed(app);
    if let Some(e) = x.layouts_error {
        alert(app, &format!("Your apps were imported, but the window layouts and scenes in the backup couldn't be:
{e}"));
    }
    if let Some(e) = x.media_error {
        alert(app, &format!("Your apps were imported, but the Media library in the backup couldn't be:
{e}"));
    }
    if !x.stored_missing.is_empty() {
        alert(
            app,
            &format!(
                "These stored files weren't found next to the backup, so they show as missing:
{}

Put the backup's \"-files\" folder beside it and import again to bring them back.",
                bullets(&x.stored_missing)
            ),
        );
    }
}

// ---------- updates ----------

/// update:check, off the UI thread. `notify` (the start-up check) also
/// announces a newer release from the tray, as 1.2 did with notifyUpdates on.
fn check_for_updates(app: &App, notify: bool) {
    st(|s| s.update = Update::Checking);
    redraw(app);
    std::thread::spawn(move || {
        let res = update::check_for_update(VERSION, services::update_fetch);
        super::post(move |app| {
            if matches!(st(|s| s.update.clone()), Update::Downloading(_)) {
                return;
            }
            let state = match res {
                Ok(Some(v)) => {
                    if notify {
                        app.notify("Launchpad update available", &format!("Version {v} is ready to install."));
                    }
                    Update::Available(v)
                }
                Ok(None) => Update::UpToDate,
                Err(e) => Update::Error(e.to_string()),
            };
            st(|s| s.update = state);
            redraw(app);
        });
    });
}

/// Downloads `version`'s installer, checks it, runs it silently and quits:
/// one path for "Download & Install" and "Reinstall This Version".
fn download_and_run(version: String, done: impl FnOnce(&Rc<App>, launchpad_core::Error) + Send + 'static) {
    std::thread::spawn(move || {
        let temp = std::env::temp_dir().join("launchpad-update");
        let res = update::download(&version, services::update_fetch, services::sha512, &temp);
        super::post(move |app| {
            // /S: silent; --force-run: start Launchpad again when it's done.
            let run = res.and_then(|installer| launch::run_installer(&installer.to_string_lossy(), "/S --force-run"));
            match run {
                Ok(()) => app.quit(),
                Err(e) => done(app, e),
            }
        });
    });
}

/// Running as the portable exe (no installer's Uninstall.exe beside it): an
/// update is a new exe to download by hand, not an installer to run, which
/// would install into Program Files.
fn portable() -> bool {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| !d.join("Uninstall.exe").is_file())).unwrap_or(true)
}

fn release_page(version: &str) -> String {
    format!("https://github.com/{}/releases/tag/v{version}", update::REPO)
}

fn install_update(app: &Rc<App>) {
    let Update::Available(v) = st(|s| s.update.clone()) else { return };
    if portable() {
        if let Err(e) = launch::open_external(&release_page(&v)) {
            alert(app, &e.to_string());
        }
        return;
    }
    st(|s| s.update = Update::Downloading(v.clone()));
    redraw(app);
    download_and_run(v.clone(), move |app, e| {
        st(|s| s.update = Update::Available(v));
        redraw(app);
        alert(app, &e.to_string());
    });
}

fn reinstall(app: &Rc<App>) {
    if portable() {
        if let Err(e) = launch::open_external(&release_page(VERSION)) {
            alert(app, &e.to_string());
        }
        return;
    }
    st(|s| s.busy[3] = true);
    redraw(app);
    download_and_run(VERSION.to_string(), |app, e| {
        st(|s| s.busy[3] = false);
        redraw(app);
        alert(app, &format!("Couldn't reinstall:\n{e}"));
    });
}

// ---------- reset ----------

fn run_reset(app: &Rc<App>) {
    if !st(|s| s.reset.as_ref().is_some_and(Reset::can_confirm)) {
        return;
    }
    st(|s| s.reset.iter_mut().for_each(|r| r.busy = true));
    redraw(app);
    // The panels give their space back first, and Media closes.
    super::media::discard();
    super::organizer::shutdown();
    super::organizer::panel::forget_docked(app);
    match reset::reset(&app.data_dir, time::now_ms()) {
        Ok(_) => {
            clear_prefs(app);
            slint::Timer::single_shot(std::time::Duration::from_millis(600), || {
                super::with(|app| app.relaunch());
            });
        }
        Err(e) => {
            st(|s| s.reset.iter_mut().for_each(|r| r.busy = false));
            redraw(app);
            alert(app, &format!("Couldn't reset Launchpad:\n{e}"));
        }
    }
}

/// clearLocalPrefs: Launchpad's own prefs go, Media's (launchpad-media-*)
/// stay. The wallpaper file goes with its pref.
fn clear_prefs(app: &App) {
    if let Some(name) = app.pref(ui_wallpaper_key()).filter(|n| is_wallpaper_name(n)) {
        let _ = std::fs::remove_file(app.data_dir.join(name));
    }
    let mut p = app.prefs.borrow_mut();
    let keys: Vec<String> = p.keys().filter(|k| k.starts_with("launchpad-") && !k.starts_with("launchpad-media-")).map(str::to_string).collect();
    for k in keys {
        p.remove(&k);
    }
    if let Err(e) = p.save(&app.data_dir) {
        eprintln!("Saving ui.json: {e}");
    }
}

/// Test steps for `modals::debug_open` that stand in for Settings' file dialogs.
#[cfg(debug_assertions)]
pub(crate) fn debug_step(app: &Rc<App>, verb: &str, arg: &str) {
    match verb {
        "wallpaper" => set_wallpaper(app, PathBuf::from(arg)),
        "export" => export_to(app, PathBuf::from(arg), false),
        "import" => import_from(app, PathBuf::from(arg)),
        "action" => action(app, arg),
        // Posted keys can't type capitals: fills the reset dialog's box.
        "reset-text" => {
            st(|s| s.reset.iter_mut().for_each(|r| r.text = arg.to_string()));
            redraw(app);
        }
        _ => eprintln!("LAUNCHPAD_DEBUG_OPEN: unknown step {verb}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::Key;

    #[test]
    fn search_matches_titles_text_and_options_across_sections() {
        let v = view(2, "");
        assert_eq!(v.shown, [false, false, true, false, false, false, false, false]);
        assert_eq!(v.total, 5);
        assert!(v.firsts[10] && !v.firsts[11]);
        let v = view(0, "  HOTKEY ");
        assert_eq!(v.counts[3], 3); // "Hotkeys" is the section's title: all three match
        assert!(v.hits[16]);
        let v = view(0, "translucent");
        assert_eq!((v.counts[1], v.counts[2], v.total), (1, 1, 2));
        assert!(v.firsts[9] && v.firsts[10]);
        assert_eq!(view(0, "zzz").total, 0);
        assert!(view(0, "slint").hits[26]);
    }

    #[test]
    fn accelerators_are_built_as_acceleratorfromevent_did() {
        let k = |k: Key| char::from(k).to_string();
        assert_eq!(accel_from_key(&k(Key::Control), true, false, false, false), None);
        assert_eq!(accel_from_key(&k(Key::F4), true, true, false, false).as_deref(), Some("CommandOrControl+Alt+F4"));
        assert_eq!(accel_from_key(" ", true, false, false, false).as_deref(), Some("CommandOrControl+Space"));
        assert_eq!(accel_from_key("g", false, true, true, false).as_deref(), Some("Alt+Shift+G"));
        assert_eq!(accel_from_key("\u{7}", true, false, false, false).as_deref(), Some("CommandOrControl+G"));
        assert_eq!(accel_from_key(&k(Key::UpArrow), false, true, false, false).as_deref(), Some("Alt+Up"));
        assert!(hotkey::parse("CommandOrControl+Alt+F4").is_some());
        assert_eq!(format_accel("CommandOrControl+Alt+F8"), "Ctrl+Alt+F8");
    }

    #[test]
    fn reset_needs_a_backup_decision_and_reset_typed() {
        let mut r = Reset { text: " RESET ".into(), ..Default::default() };
        assert!(!r.can_confirm());
        r.no_backup = true;
        assert!(r.can_confirm());
        r.text = "reset".into();
        assert!(!r.can_confirm());
        assert!(is_wallpaper_name("wallpaper.png") && !is_wallpaper_name("wallpaper.") && !is_wallpaper_name("..\\x") && !is_wallpaper_name("wallpaper.p/g"));
    }
}
