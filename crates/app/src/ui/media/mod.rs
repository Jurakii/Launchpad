//! Media (renderer/media/ and main.js's media handlers in 1.2): a fullscreen
//! window over library.json with its own screens: Home, Add (a single app or
//! a library folder), Settings, Customize art and Running.
//!
//! Rust owns the focus. Every focusable element in ui/media.slint is a `Nav`
//! with an id that reports its rectangle (`report`); the arrow keys, Tab and
//! a pad's D-pad move to the nearest one (focus.rs), and Enter, a click or A
//! runs `activate` for that id. A text field keeps the keyboard while it's
//! focused and hands Up, Down and Tab back.
//!
//! The window lives from Open to Exit; the hotkey only hides and shows it.
//! The launcher outlives it, so a game started from Media is still tracked
//! (and its play time recorded) after Media closes.
mod art;
mod focus;
pub(crate) mod forms;
mod pad;
mod settings;

use super::{post, App, MPrompt, MRow, MTile, MediaState, MediaTheme, MediaWindow};
use crate::win::{self, services::MediaHost};
use focus::{Dir, Rect};
use launchpad_core::media::controller::{self, Controller};
use launchpad_core::media::launch::{self, Launched, Launcher};
use launchpad_core::media::library::{Entry, Library};
use launchpad_core::media::pad::{default_mapping, Mapping, Pad, PadReader};
use launchpad_core::media::players::{self, Profile, ProfilesData};
use launchpad_core::media::{art as core_art, scan, theme};
use launchpad_core::time::now_ms;
use serde_json::Value;
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

pub const LIBRARY_DEFAULTS_KEY: &str = "launchpad-media-library-defaults";
const TAB_ORDER: [&str; 3] = ["games", "videos", "all"];
// Gamepad button numbers (standard mapping) as people call them.
const BUTTON_NAMES: [&str; 17] = [
    "A", "B", "X", "Y", "LB", "RB", "LT", "RT", "Back", "Start", "Left stick", "Right stick", "D-pad up", "D-pad down", "D-pad left",
    "D-pad right", "Guide",
];

pub fn button_name(b: u32) -> String {
    BUTTON_NAMES.get(b as usize).map_or_else(|| format!("Button {}", b + 1), |s| s.to_string())
}

pub(crate) struct Running {
    id: String,
    name: String,
    with: String,
    started_at: i64,
}

struct Capture {
    action: String,
    /// Buttons held at the last frame, per pad: only a new press counts.
    held: HashMap<String, Vec<u32>>,
    _timeout: Timer,
}

pub(crate) struct Media {
    win: MediaWindow,
    hwnd: Option<HWND>,
    screen: &'static str,
    tab: &'static str,
    entries: Vec<Entry>,
    profiles: ProfilesData,
    detected: Vec<Profile>,
    controller: Controller,
    focus: String,
    home_focus: Option<String>,
    /// The entry the home hero shows.
    hero: String,
    rects: HashMap<String, (Rect, i32)>,
    epoch: i32,
    pad_input: bool,
    running: Option<Running>,
    art: art::Cache,
    /// Home's rows: each row's tile models and the entry ids in them, so art
    /// that arrives can be put in place without rebuilding the rows.
    tiles: Vec<(Rc<VecModel<MTile>>, Vec<String>, bool)>,
    toast_timer: Timer,
    _clock_timer: Timer,
    run_timer: Timer,
    pad_timer: Timer,
    pads: pad::Pads,
    reader: PadReader,
    front: bool,
    /// The connected pads' ids, in slot order.
    pad_ids: Vec<String>,
    /// Pad events found inside the state borrow, run after it.
    actions_pending: Vec<String>,
    capture: Option<Capture>,
    add: forms::Add,
    app_form: forms::AddApp,
    customizing: Option<String>,
    section: &'static str,
    editing: Option<settings::Editing>,
    edits: HashMap<String, String>,
}

thread_local! {
    static MEDIA: RefCell<Option<Media>> = const { RefCell::new(None) };
    // Outlive the window: tracked launches, and library.json's read-only state.
    static LAUNCHER: RefCell<Option<Launcher<MediaHost>>> = const { RefCell::new(None) };
    static LIBRARY: RefCell<Option<Library>> = const { RefCell::new(None) };
}

/// Runs `f` on Media's state while the window is open.
fn md<R>(f: impl FnOnce(&mut Media) -> R) -> Option<R> {
    MEDIA.with_borrow_mut(|m| m.as_mut().map(f))
}

fn st(m: &Media) -> MediaState<'_> {
    m.win.global::<MediaState>()
}

fn library<R>(app: &App, f: impl FnOnce(&mut Library) -> R) -> R {
    LIBRARY.with_borrow_mut(|l| f(l.get_or_insert_with(|| Library::new(&app.data_dir))))
}

fn launcher<R>(f: impl FnOnce(&mut Launcher<MediaHost>) -> R) -> R {
    LAUNCHER.with_borrow_mut(|l| {
        f(l.get_or_insert_with(|| {
            Launcher::new(MediaHost { on_exit: Arc::new(|pid, code| post(move |app| on_exit(app, pid, code))) })
        }))
    })
}

pub fn is_open() -> bool {
    MEDIA.with_borrow(|m| m.is_some())
}

/// The library, or an empty one with a message for the toast.
fn load_entries(app: &App) -> (Vec<Entry>, Option<String>) {
    library(app, |l| match l.load() {
        Ok(e) => {
            let issue = l.get_load_issue().filter(|i| i.state == launchpad_core::media::library::LoadState::ReadOnly);
            (e, issue.map(|_| "library.json couldn't be read, so changes won't be saved until it is fixed.".to_string()))
        }
        Err(e) => (Vec::new(), Some(e.to_string())),
    })
}

fn platform_name(id: Option<&str>) -> String {
    match id {
        None => "PC & other games".into(),
        Some(id) => scan::platform_by_id(id).map_or_else(|| id.to_string(), |p| p.name.to_string()),
    }
}

/// localeCompare(a, b, undefined, { sensitivity: 'base', numeric: true }), by Windows' own collation.
fn compare_names(a: &str, b: &str) -> std::cmp::Ordering {
    use windows::Win32::Globalization::{CompareStringEx, LINGUISTIC_IGNORECASE, SORT_DIGITSASNUMBERS};
    let (wa, wb): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    match unsafe { CompareStringEx(None, LINGUISTIC_IGNORECASE | SORT_DIGITSASNUMBERS, &wa, &wb, None, None, None) }.0 {
        1 => std::cmp::Ordering::Less,
        3 => std::cmp::Ordering::Greater,
        2 => std::cmp::Ordering::Equal,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

pub fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        return "under a minute".into();
    }
    let (h, m) = (seconds / 3600, (seconds % 3600) / 60);
    if h > 0 { format!("{h}h {m:02}m") } else { format!("{m}m") }
}

fn num(e: &Entry, key: &str) -> f64 {
    e.0.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

// ---------- Open, close, the hotkey ----------

/// The Media button: opens Media, or brings it forward.
pub fn open(app: &Rc<App>) {
    if let Some(h) = md(|m| {
        let _ = m.win.show();
        m.hwnd
    }) {
        if let Some(h) = h {
            unsafe {
                let _ = SetForegroundWindow(h);
            }
        }
        return;
    }
    let win = match MediaWindow::new() {
        Ok(w) => w,
        Err(e) => return eprintln!("Couldn't open Media: {e}"),
    };
    let (entries, issue) = load_entries(app);
    let controller = controller::load(&app.data_dir);
    let mut tab = "games";
    if !entries.iter().any(|e| e.library() == "games") && entries.iter().any(|e| e.library() == "videos") {
        tab = "videos";
    }
    let reader = PadReader::new(None, Some(mapping_for(&controller)), Some(controller.dead_zone));
    let media = Media {
        win: win.clone_strong(),
        hwnd: None,
        screen: "home",
        tab,
        entries,
        profiles: players::load(&app.data_dir).unwrap_or_default(),
        detected: Vec::new(),
        controller,
        focus: String::new(),
        home_focus: None,
        hero: String::new(),
        rects: HashMap::new(),
        epoch: 0,
        pad_input: false,
        running: None,
        art: art::Cache::default(),
        tiles: Vec::new(),
        toast_timer: Timer::default(),
        _clock_timer: clock_timer(),
        run_timer: Timer::default(),
        pad_timer: Timer::default(),
        pads: pad::Pads::default(),
        reader,
        front: false,
        pad_ids: Vec::new(),
        actions_pending: Vec::new(),
        capture: None,
        add: forms::Add::default(),
        app_form: forms::AddApp::default(),
        customizing: None,
        section: "profiles",
        editing: None,
        edits: HashMap::new(),
    };
    MEDIA.with_borrow_mut(|m| *m = Some(media));
    apply_theme(app);
    bind(&win);
    set_clock();
    win.global::<MediaState>().set_platform_options(
        ModelRc::new(VecModel::from(std::iter::once("Other games".into()).chain(scan::PLATFORMS.iter().map(|p| p.name.into())).collect::<Vec<_>>())),
    );
    win.global::<MediaState>().set_group_options(
        ModelRc::new(VecModel::from(std::iter::once("Not detected".into()).chain(scan::PLATFORMS.iter().map(|p| p.name.into())).collect::<Vec<_>>())),
    );
    // On the display Launchpad is on, fullscreen as 1.2 opened it.
    if let Some(pos) = app.window.borrow().as_ref().map(|w| w.window().position()) {
        win.window().set_position(pos);
    }
    win.window().set_fullscreen(true);
    win.window().on_close_requested(|| {
        super::later(close);
        slint::CloseRequestResponse::KeepWindowShown
    });
    render_home(app);
    let running = launcher(|l| l.list()).into_iter().next();
    match running {
        Some(r) => {
            let with = md(|m| m.entries.iter().find(|e| e.id() == r.id).and_then(|e| players::profile_for(e, &m.profiles)).map(|p| p.name.clone()))
                .flatten()
                .unwrap_or_default();
            md(|m| m.running = Some(Running { id: r.id, name: r.name, with, started_at: r.started_at }));
            show_running(app);
        }
        None => show_screen(app, "home"),
    }
    let _ = win.show();
    super::shell::when_ready(&win, |hwnd| {
        md(|m| m.hwnd = Some(hwnd));
        forms::register_drop(hwnd);
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        win_grab_keys();
        fit_to_screen(hwnd);
    });
    start_pad_timer();
    if let Some(issue) = issue {
        toast(&issue);
    }
}

/// Media is a 10-foot screen: it's laid out for 1080 lines and drawn
/// bigger on a taller display (1440p: a third bigger), never smaller than
/// the display's own scale, times Settings → Interface size. Art is decoded
/// again at the new size.
fn fit_to_screen(hwnd: HWND) {
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    let Some(mon) = crate::win::display::info(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }) else { return };
    let tall = (mon.bounds.bottom - mon.bounds.top) as f64 / 1080.0;
    let ui = super::with(|app| app.settings().get("uiScale").and_then(Value::as_f64)).flatten().unwrap_or(1.0);
    let s = (mon.scale.max(tall) * ui) as f32;
    let changed = md(|m| {
        let differs = (scale(m) - s).abs() > 0.01;
        if differs {
            m.win.window().dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: s });
        }
        differs
    });
    if changed == Some(true) {
        super::later(render_home);
    }
}

/// Exit (or Back on Home, or the window closing): Media goes and Launchpad comes back.
pub fn close(app: &Rc<App>) {
    let Some(m) = MEDIA.with_borrow_mut(Option::take) else { return };
    // Hide first: dropping a shown window leaks its buffer.
    let _ = m.win.hide();
    drop(m);
    app.show(true);
}

/// Closes Media without bringing Launchpad back (Reset, quitting).
pub fn discard() {
    if let Some(m) = MEDIA.with_borrow_mut(Option::take) {
        let _ = m.win.hide();
    }
}

/// The hotkey while Media is open: it brings Media back from a game, and
/// hides it when it's already in front. False when Media isn't open.
pub fn toggle(_app: &Rc<App>) -> bool {
    let Some((win, hwnd)) = md(|m| (m.win.clone_strong(), m.hwnd)) else { return false };
    let front = hwnd.is_some_and(|h| unsafe { GetForegroundWindow() } == h);
    if front && win.window().is_visible() {
        let _ = win.hide();
    } else {
        let _ = win.show();
        if let Some(h) = hwnd {
            unsafe {
                let _ = SetForegroundWindow(h);
            }
        }
    }
    true
}

/// Launchpad's theme or accent changed: Media follows (its own colours stay).
pub fn theme_changed(app: &Rc<App>) {
    if is_open() {
        apply_theme(app);
    }
}

fn apply_theme(app: &App) {
    let overrides = settings::color_overrides(app);
    let theme_name = app.pref("launchpad-theme").unwrap_or_default();
    let values = theme::effective(&overrides, &theme_name, app.pref("launchpad-accent").as_deref());
    md(|m| {
        let t = m.win.global::<MediaTheme>();
        for (token, value) in &values {
            let Some(c) = super::prefs::parse_hex(value) else { continue };
            match *token {
                "--bg" => t.set_bg(c),
                "--panel" => t.set_panel(c),
                "--raised" => t.set_raised(c),
                "--border" => t.set_border(c),
                "--text" => t.set_text(c),
                "--muted" => t.set_muted(c),
                "--accent" => t.set_accent(c),
                "--on-accent" => t.set_on_accent(c),
                "--warn" => t.set_warn(c),
                "--danger" => t.set_danger(c),
                "--shadow" => t.set_shadow(c),
                _ => {}
            }
        }
    });
}

fn clock_timer() -> Timer {
    let t = Timer::default();
    t.start(TimerMode::Repeated, Duration::from_secs(30), set_clock);
    t
}

fn set_clock() {
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
    let mut buf = [0u16; 64];
    let n = unsafe { GetTimeFormatEx(None, TIME_NOSECONDS, None, None, Some(&mut buf)) };
    let text = win::from_wide(&buf[..n.max(0) as usize]);
    md(|m| st(m).set_clock(text.into()));
}

fn win_grab_keys() {
    if let Some(w) = md(|m| m.win.clone_strong()) {
        w.invoke_grab_keys();
    }
}

pub fn toast(text: &str) {
    md(|m| {
        st(m).set_toast(text.into());
        let w = m.win.as_weak();
        m.toast_timer.start(TimerMode::SingleShot, Duration::from_secs(4), move || {
            if let Some(w) = w.upgrade() {
                w.global::<MediaState>().set_toast("".into());
            }
        });
    });
}

// ---------- The window's callbacks ----------

fn bind(win: &MediaWindow) {
    let s = win.global::<MediaState>();
    s.on_report(|id, x, y, w, h| {
        if id.is_empty() {
            return;
        }
        md(|m| {
            if w <= 0.0 || h <= 0.0 {
                m.rects.remove(id.as_str());
            } else {
                m.rects.insert(id.to_string(), (Rect { x, y, w, h }, m.epoch));
            }
        });
    });
    s.on_click(|id| {
        let id = id.to_string();
        set_input(false);
        super::later(move |app| {
            set_focus(app, &id);
            activate(app, &id);
        });
    });
    s.on_set_focus(|id| {
        let id = id.to_string();
        super::later(move |app| set_focus(app, &id));
    });
    s.on_key(|key, _shift| on_key(key.as_str()));
    s.on_edited(|id, text| {
        md(|m| m.edits.insert(id.to_string(), text.to_string()));
    });
    s.on_accepted(|id| {
        let id = id.to_string();
        super::later(move |app| forms::accepted(app, &id));
    });
    s.on_pick(|id, i| {
        let id = id.to_string();
        super::later(move |app| forms::pick(app, &id, i as usize));
    });
}

fn set_input(pad: bool) {
    let changed = md(|m| std::mem::replace(&mut m.pad_input, pad) != pad).unwrap_or(false);
    if changed {
        render_prompts();
    }
}

fn is_field(id: &str) -> bool {
    matches!(id, "app:name" | "pf:name" | "pf:exe" | "pf:args" | "pf:platforms") || id.starts_with("color:") || id.starts_with("slot-url:")
}

/// What the window's (or a field's) key does. True if it was used.
fn on_key(key: &str) -> bool {
    set_input(false);
    let Some((focus, capturing, screen)) = md(|m| (m.focus.clone(), m.capture.is_some(), m.screen)) else { return false };
    if capturing && key != "escape" {
        return true; // a pad button is being waited for
    }
    match key {
        "up" | "down" | "left" | "right" => {
            // A multi-line field with several lines keeps Up and Down.
            if focus == "pf:args" && md(|m| st(m).get_pf_args().contains('\n')).unwrap_or(false) {
                return false;
            }
            super::with(|app| move_focus(app, Dir::parse(key).unwrap_or(Dir::Down))).unwrap_or(false)
        }
        "tab" => super::with(|app| move_focus(app, Dir::Down) || move_focus(app, Dir::Right)).unwrap_or(false),
        "backtab" => super::with(|app| move_focus(app, Dir::Up) || move_focus(app, Dir::Left)).unwrap_or(false),
        "enter" => {
            if is_field(&focus) || focus.is_empty() {
                return false;
            }
            super::later(move |app| activate(app, &focus));
            true
        }
        "escape" => {
            super::later(back);
            true
        }
        "c" if screen == "home" && focus.starts_with("tile:") => {
            super::later(move |app| forms::open_customize(app, tile_entry(&focus)));
            true
        }
        _ => false,
    }
}

/// The entry id in a tile's nav id ("tile:<row>:<id>").
fn tile_entry(nav: &str) -> &str {
    nav.splitn(3, ':').nth(2).unwrap_or("")
}

/// Esc, B: out of a capture, else back to Home, else out of Media.
fn back(app: &Rc<App>) {
    if md(|m| m.capture.is_some()).unwrap_or(false) {
        return settings::cancel_capture(app);
    }
    match md(|m| m.screen) {
        Some("home") => close(app),
        Some(_) => show_screen(app, "home"),
        None => {}
    }
}

// ---------- Focus ----------

/// Bumped whenever what's on screen changes, so each Nav reports again and
/// ones that are gone stop counting.
fn bump(m: &mut Media) {
    m.epoch += 1;
    st(m).set_epoch(m.epoch);
}

fn area(id: &str) -> u8 {
    u8::from(id.starts_with("tab:") || id.starts_with("top:"))
}

fn move_focus(app: &Rc<App>, dir: Dir) -> bool {
    let target = md(|m| {
        let items: Vec<(&str, Rect)> = m
            .rects
            .iter()
            .filter(|(id, (_, e))| *e == m.epoch && id.as_str() != "page" && !id.starts_with("drop:"))
            .map(|(id, (r, _))| (id.as_str(), *r))
            .collect();
        match m.rects.get(&m.focus).filter(|(_, e)| *e == m.epoch) {
            Some((from, _)) => focus::step(&items, &m.focus, *from, dir, area).map(str::to_string),
            None => Some(first_of(m)),
        }
    })
    .flatten();
    match target {
        Some(t) => {
            set_focus(app, &t);
            true
        }
        None => false,
    }
}

/// Where the focus starts on each screen.
fn first_of(m: &Media) -> String {
    match m.screen {
        "home" => m.tiles.first().and_then(|(_, ids, _)| ids.first().map(|id| format!("tile:0:{id}"))).unwrap_or_else(|| "home:add".into()),
        "addChoice" => "choice:single".into(),
        "addApp" => "app:browse".into(),
        "add" => "add:browse".into(),
        "settings" => format!("sec:{}", m.section),
        "customize" => "slot-file:cover".into(),
        "running" => "run:stop".into(),
        _ => String::new(),
    }
}

pub fn set_focus(app: &Rc<App>, id: &str) {
    let Some(field) = md(|m| {
        m.focus = id.to_string();
        st(m).set_focus(id.into());
        if m.screen == "home" && id.starts_with("tile:") {
            m.home_focus = Some(id.to_string());
        }
        is_field(id)
    }) else {
        return;
    };
    if !field {
        win_grab_keys();
    }
    if id.starts_with("tile:") {
        show_hero(app, tile_entry(id));
    }
    // After the window has laid out what may just have appeared.
    let id = id.to_string();
    super::later(move |_| reveal(&id));
}

/// Scrolls the screen so the focused element is in it.
fn reveal(id: &str) {
    md(|m| {
        if area(id) == 1 {
            return;
        }
        let (Some((page, _)), Some((r, _))) = (m.rects.get("page"), m.rects.get(id)) else { return };
        const MARGIN: f32 = 24.0;
        let s = st(m);
        let y = s.get_page_y();
        if r.y < page.y + MARGIN {
            s.set_page_y((y + page.y + MARGIN - r.y).min(0.0));
        } else if r.y + r.h > page.y + page.h - MARGIN {
            s.set_page_y(y - (r.y + r.h - (page.y + page.h - MARGIN)));
        }
    });
}

// ---------- Screens ----------

pub fn show_screen(app: &Rc<App>, name: &'static str) {
    let target = md(|m| {
        m.screen = name;
        let s = st(m);
        s.set_screen(name.into());
        s.set_page_y(0.0);
        bump(m);
        match name {
            "home" => m.home_focus.clone().filter(|f| m.tiles.iter().enumerate().any(|(ri, (_, ids, _))| ids.iter().any(|id| *f == format!("tile:{ri}:{id}")))),
            _ => None,
        }
        .unwrap_or_else(|| first_of(m))
    });
    render_prompts();
    if let Some(t) = target {
        set_focus(app, &t);
    }
}

fn render_prompts() {
    md(|m| {
        let pad = m.pad_input;
        let mapping = active_mapping(m);
        let name_of = |action: &str| mapping.iter().find(|(_, a)| *a == action).map(|(b, _)| button_name(*b));
        let glyph = |action: &str| -> Option<String> {
            if !pad {
                return match action {
                    "select" => Some("Enter".into()),
                    "move" => Some("Arrows".into()),
                    "back" => Some("Esc".into()),
                    "customize" => Some("C".into()),
                    _ => None,
                };
            }
            match action {
                "move" => Some("D-pad".into()),
                "tabs" => Some(format!("{} / {}", name_of("tabPrev")?, name_of("tabNext")?)),
                other => name_of(other),
            }
        };
        let basic: &[(&str, &str)] = &[("select", "Select"), ("move", "Move"), ("back", "Back")];
        let list: &[(&str, &str)] = match m.screen {
            "home" => &[("select", "Launch"), ("move", "Move"), ("customize", "Customize art"), ("tabs", "Switch tab"), ("menu", "Settings"), ("back", "Exit Media")],
            "settings" => &[("select", "Select"), ("move", "Move"), ("tabs", "Switch section"), ("back", "Back")],
            "running" => &[("select", "Close it"), ("back", "Back to library")],
            _ => basic,
        };
        let prompts: Vec<MPrompt> =
            list.iter().filter_map(|(a, label)| glyph(a).map(|g| MPrompt { glyph: g.into(), label: (*label).into() })).collect();
        st(m).set_prompts(ModelRc::new(VecModel::from(prompts)));
    });
}

// ---------- Home ----------

/// homeRows(): Continue, then each platform's games, then videos.
fn home_rows(m: &Media) -> Vec<(String, bool, Vec<usize>)> {
    let shown: Vec<usize> = (0..m.entries.len()).filter(|&i| m.tab == "all" || m.entries[i].library() == m.tab).collect();
    let e = |i: usize| &m.entries[i];
    let mut rows = Vec::new();
    let mut played: Vec<usize> = shown.iter().copied().filter(|&i| num(e(i), "launchCount") > 0.0).collect();
    played.sort_by(|&a, &b| num(e(b), "lastLaunched").total_cmp(&num(e(a), "lastLaunched")));
    played.truncate(12);
    if !played.is_empty() {
        rows.push(("Continue".to_string(), false, played));
    }
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for &i in shown.iter().filter(|&&i| e(i).library() == "games") {
        let key = e(i).platform().unwrap_or("").to_string();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| match (a.is_empty(), b.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => compare_names(&platform_name(Some(a)), &platform_name(Some(b))),
    });
    for (key, mut list) in groups {
        list.sort_by(|&a, &b| compare_names(e(a).name(), e(b).name()));
        rows.push((platform_name(Some(&key).filter(|k| !k.is_empty()).map(String::as_str)), false, list));
    }
    let videos: Vec<usize> = shown.iter().copied().filter(|&i| e(i).library() == "videos").collect();
    if !videos.is_empty() {
        let mut recent = videos.clone();
        recent.sort_by(|&a, &b| num(e(b), "createdAt").total_cmp(&num(e(a), "createdAt")));
        recent.truncate(24);
        rows.push(("Recently added".to_string(), true, recent));
        let mut all = videos;
        all.sort_by(|&a, &b| compare_names(e(a).name(), e(b).name()));
        rows.push(("All videos".to_string(), true, all));
    }
    rows
}

/// The art file of one slot and its version, if the file is there.
fn art_of(app: &App, e: &Entry, slot: &str) -> Option<(std::path::PathBuf, i64)> {
    let present = core_art::existing_art(&app.data_dir, e);
    let v = present.get(slot)?.get("updatedAt").and_then(Value::as_i64).unwrap_or(0);
    Some((core_art::art_path(&app.data_dir, e.id(), slot).ok()?, v))
}

fn scale(m: &Media) -> f32 {
    m.win.window().scale_factor()
}

/// An art image at `w`×`h` logical px, or None while it's being decoded.
fn image(app: &App, m: &mut Media, e: &Entry, slots: &[&str], w: f32, h: f32, cover: bool) -> Option<Image> {
    let (path, version) = slots.iter().find_map(|s| art_of(app, e, s))?;
    let s = scale(m);
    m.art.get(art::Key { path, version, w: (w * s).round() as u32, h: (h * s).round() as u32, cover })
}

fn tile_for(app: &App, m: &mut Media, ri: usize, i: usize, wide_row: bool) -> MTile {
    let e = m.entries[i].clone();
    let wide = wide_row || e.library() == "videos";
    let slots: &[&str] = if wide { &["banner", "screenshot", "cover"] } else { &["cover", "banner", "screenshot"] };
    let (w, h) = if wide { (220.0, 124.0) } else { (150.0, 210.0) };
    let art = image(app, m, &e, slots, w, h, true);
    MTile {
        nav: format!("tile:{ri}:{}", e.id()).into(),
        name: e.name().into(),
        has_art: art.is_some(),
        art: art.unwrap_or_default(),
        available: e.available(),
        wide,
    }
}

pub fn render_home(app: &Rc<App>) {
    md(|m| {
        let rows = home_rows(m);
        let mut models = Vec::new();
        let mut out = Vec::new();
        for (ri, (title, wide, list)) in rows.iter().enumerate() {
            let tiles: Vec<MTile> = list.iter().map(|&i| tile_for(app, m, ri, i, *wide)).collect();
            let model = Rc::new(VecModel::from(tiles));
            out.push(MRow { title: title.as_str().into(), wide: *wide, tiles: model.clone().into() });
            models.push((model, list.iter().map(|&i| m.entries[i].id().to_string()).collect(), *wide));
        }
        m.tiles = models;
        let s = st(m);
        s.set_tab(m.tab.into());
        s.set_empty(out.is_empty());
        s.set_empty_title(
            match m.tab {
                "videos" => "No videos yet",
                "games" => "No games yet",
                _ => "Nothing here yet",
            }
            .into(),
        );
        s.set_rows(ModelRc::new(VecModel::from(out)));
        bump(m);
    });
    // The hero shows the focused tile, or the first one.
    let first = md(|m| {
        let focused = m.home_focus.clone().filter(|f| m.tiles.iter().enumerate().any(|(ri, (_, ids, _))| ids.iter().any(|id| *f == format!("tile:{ri}:{id}"))));
        focused.map(|f| tile_entry(&f).to_string()).or_else(|| m.tiles.first().and_then(|(_, ids, _)| ids.first().cloned()))
    })
    .flatten();
    if let Some(id) = first {
        show_hero(app, &id);
    }
    start_art();
}

fn show_hero(app: &Rc<App>, id: &str) {
    md(|m| {
        let Some(e) = m.entries.iter().find(|e| e.id() == id).cloned() else { return };
        let mut info = vec![if e.library() == "videos" { "Video".to_string() } else { platform_name(e.platform()) }];
        if let Some(y) = e.0.get("meta").and_then(|v| v.get("year")).filter(|v| launchpad_core::truthy(Some(v))) {
            info.push(y.as_str().map_or_else(|| y.to_string(), str::to_string));
        }
        let count = num(&e, "launchCount") as i64;
        if count > 0 {
            info.push(if count == 1 { "Played once".into() } else { format!("Played {count} times") });
        }
        let secs = num(&e, "playSeconds") as i64;
        if secs >= 60 {
            info.push(format!("{} in total", format_duration(secs)));
        }
        if !e.available() {
            info.push("File not found: rescan the folder or check the drive".into());
        }
        if e.available() && e.launch_type() == "profile" {
            info.push(match players::profile_for(&e, &m.profiles) {
                Some(p) => format!("Opens in {}", p.name),
                None => "No player or emulator set up".into(),
            });
        }
        let logo = image(app, m, &e, &["logo"], 640.0, 200.0, false);
        // A logo still decoding: show no title yet rather than the text, then the logo.
        let waiting = logo.is_none() && art_of(app, &e, "logo").is_some_and(|(path, v)| !m.art.failed(&path, v));
        let size = m.win.window().size().to_logical(scale(m));
        let backdrop = image(app, m, &e, &["banner"], size.width, size.height, true);
        // The neighbours' logos too, so moving along a row shows them at once.
        let near: Vec<Entry> = m
            .tiles
            .iter()
            .find_map(|(_, ids, _)| {
                let i = ids.iter().position(|x| x == id)?;
                Some(ids[i.saturating_sub(2)..(i + 3).min(ids.len())].iter().filter_map(|x| m.entries.iter().find(|e| e.id() == x).cloned()).collect())
            })
            .unwrap_or_default();
        for n in &near {
            image(app, m, n, &["logo"], 640.0, 200.0, false);
        }
        m.hero = id.to_string();
        let s = st(m);
        s.set_hero_name(if waiting { "".into() } else { e.name().into() });
        s.set_hero_info(info.join(" · ").into());
        s.set_hero_has_logo(logo.is_some());
        s.set_hero_logo(logo.unwrap_or_default());
        s.set_backdrop(backdrop.unwrap_or_default());
    });
    start_art();
}

/// Decodes whatever art was asked for, then draws it in.
fn start_art() {
    let Some(jobs) = md(|m| m.art.take_jobs()).flatten() else { return };
    std::thread::spawn(move || {
        let results = art::decode(jobs);
        post(move |app| art_done(app, results));
    });
}

fn art_done(app: &Rc<App>, results: Vec<(art::Key, Option<crate::win::image::Rgba>)>) {
    md(|m| m.art.done(results));
    // The tiles in place (rebuilding the rows would lose each one's scroll).
    md(|m| {
        for ri in 0..m.tiles.len() {
            let (model, ids, wide) = m.tiles[ri].clone();
            for (ti, id) in ids.iter().enumerate() {
                let Some(i) = m.entries.iter().position(|e| e.id() == id) else { continue };
                let tile = tile_for(app, m, ri, i, wide);
                if slint::Model::row_data(&*model, ti).is_some_and(|t| t.has_art != tile.has_art) {
                    slint::Model::set_row_data(&*model, ti, tile);
                }
            }
        }
    });
    let (hero, screen) = md(|m| (m.hero.clone(), m.screen)).unwrap_or_default();
    if !hero.is_empty() {
        show_hero(app, &hero);
    }
    if screen == "customize" {
        forms::render_customize(app);
    }
    start_art();
}

fn switch_tab(app: &Rc<App>, step: i32) {
    let Some(tab) = md(|m| {
        let i = TAB_ORDER.iter().position(|t| *t == m.tab).unwrap_or(0) as i32;
        TAB_ORDER[(i + step).rem_euclid(3) as usize]
    }) else {
        return;
    };
    set_tab(app, tab);
}

fn set_tab(app: &Rc<App>, tab: &'static str) {
    md(|m| {
        m.tab = tab;
        m.home_focus = None;
    });
    render_home(app);
    if md(|m| m.screen == "home").unwrap_or(false) {
        let first = md(|m| first_of(m)).unwrap_or_default();
        set_focus(app, &first);
    }
}

/// Reloads library.json and draws Home again (after a launch, an add, art).
pub fn reload(app: &Rc<App>) {
    let (entries, _) = load_entries(app);
    md(|m| m.entries = entries);
    render_home(app);
}

// ---------- What each id does ----------

fn activate(app: &Rc<App>, id: &str) {
    let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
    match (kind, rest) {
        ("tab", t) => {
            if let Some(tab) = TAB_ORDER.iter().find(|x| **x == t) {
                set_tab(app, tab);
            }
        }
        ("top", "add") | ("home", "add") => show_screen(app, "addChoice"),
        ("top", "settings") => settings::open(app, None),
        ("top", "exit") => close(app),
        ("tile", _) => launch_entry(app, tile_entry(id)),
        ("back", _) => show_screen(app, "home"),
        ("run", "stop") => {
            if let Some(rid) = md(|m| m.running.as_ref().map(|r| r.id.clone())).flatten() {
                launcher(|l| l.stop(&rid));
            }
        }
        ("sec", _) | ("profile", _) | ("preset", _) | ("pf", _) | ("reset", _) | ("colors", _) | ("map", _) | ("dz", _) | ("lib", _) => {
            settings::activate(app, kind, rest)
        }
        _ => forms::activate(app, kind, rest),
    }
}

/// A pad action (core::media::pad), as onPadAction did.
fn on_pad_action(app: &Rc<App>, action: &str) {
    let (screen, focus, capturing) = md(|m| (m.screen, m.focus.clone(), m.capture.is_some())).unwrap_or_default();
    match action {
        "up" | "down" | "left" | "right" => {
            move_focus(app, Dir::parse(action).unwrap_or(Dir::Down));
        }
        "pageUp" | "pageDown" => {
            for _ in 0..3 {
                move_focus(app, if action == "pageUp" { Dir::Up } else { Dir::Down });
            }
        }
        // A on a dropdown steps to its next option; text needs a keyboard.
        "select" => {
            if focus.is_empty() {
                move_focus(app, Dir::Down);
            } else if !is_field(&focus) {
                activate(app, &focus);
            }
        }
        "back" => back(app),
        "tabPrev" | "tabNext" => {
            let step = if action == "tabNext" { 1 } else { -1 };
            if screen == "home" {
                switch_tab(app, step);
            } else if screen == "settings" && !capturing {
                settings::switch_section(app, step);
            }
        }
        "menu" if screen == "home" => settings::open(app, None),
        "customize" if screen == "home" && focus.starts_with("tile:") => forms::open_customize(app, tile_entry(&focus)),
        _ => {} // quick (X) arrives with the scraper's quick actions
    }
}

// ---------- Launching ----------

fn launch_entry(app: &Rc<App>, id: &str) {
    let (entries, _) = load_entries(app);
    let Some(entry) = entries.iter().find(|e| e.id() == id).cloned() else {
        return toast("That item is no longer in the library");
    };
    let profiles = players::load(&app.data_dir).unwrap_or_default();
    let result = launcher(|l| l.launch(&entry, &profiles));
    let with = players::profile_for(&entry, &profiles).map(|p| p.name.clone()).unwrap_or_default();
    match result {
        Err(e) => toast(&e.to_string()),
        Ok(Launched::AlreadyRunning) => {
            md(|m| {
                if m.running.as_ref().is_none_or(|r| r.id != id) {
                    m.running = Some(Running { id: id.into(), name: entry.name().into(), with, started_at: now_ms() });
                }
            });
            show_running(app);
        }
        Ok(l) => {
            let saved = library(app, |lib| -> launchpad_core::Result<()> {
                let list = lib.load()?;
                lib.save(&launch::record_launch(&list, id, now_ms()))
            });
            if let Err(e) = saved {
                // It launched; a library that can't be saved right now shouldn't say otherwise.
                eprintln!("Failed to record launch: {e}");
            }
            if let Launched::Tracked { .. } = l {
                md(|m| m.running = Some(Running { id: id.into(), name: entry.name().into(), with, started_at: now_ms() }));
                show_running(app);
            } else {
                toast(&format!("Started {}", entry.name()));
            }
            reload(app);
        }
    }
}

fn show_running(app: &Rc<App>) {
    let hotkey = app.settings().get("hotkey").and_then(Value::as_str).filter(|h| !h.is_empty()).unwrap_or(super::DEFAULT_HOTKEY).replace("CommandOrControl", "Ctrl");
    md(|m| {
        let Some(r) = &m.running else { return };
        let s = st(m);
        s.set_run_name(r.name.as_str().into());
        s.set_run_with(if r.with.is_empty() { "Running".into() } else { format!("Running in {}", r.with).into() });
        s.set_run_hotkey(format!("Press {hotkey} to come back to Media while it runs.").into());
        tick_running(m);
        m.run_timer.start(TimerMode::Repeated, Duration::from_secs(15), || {
            md(tick_running);
        });
    });
    show_screen(app, "running");
}

fn tick_running(m: &mut Media) {
    if let Some(r) = &m.running {
        let secs = ((now_ms() - r.started_at) as f64 / 1000.0).round() as i64;
        st(m).set_run_time(format!("Running for {}", format_duration(secs)).into());
    }
}

/// A tracked process ended (on the UI thread, from the launcher's waiter).
fn on_exit(app: &Rc<App>, pid: u32, code: Option<i32>) {
    let Some(id) = launcher(|l| l.list().into_iter().find(|r| r.pid == pid).map(|r| r.id)) else { return };
    let Some(info) = launcher(|l| l.exited(&id, code)) else { return };
    let saved = library(app, |lib| -> launchpad_core::Result<()> {
        let list = lib.load()?;
        lib.save(&launch::record_play(&list, &info.id, info.seconds, info.handoff))
    });
    if let Err(e) = saved {
        eprintln!("Failed to record play time: {e}");
    }
    let Some((was_running, name, screen)) = md(|m| {
        let was = m.running.as_ref().is_some_and(|r| r.id == info.id);
        let name = if was { m.running.as_ref().map(|r| r.name.clone()).unwrap_or_default() } else { "It".into() };
        if was {
            m.running = None;
            m.run_timer.stop();
        }
        (was, name, m.screen)
    }) else {
        return;
    };
    // A hand-off passed the game to another program, which keeps it running.
    if info.handoff {
        toast(&format!("{name} was handed to another program"));
    }
    reload(app);
    if was_running && screen == "running" {
        show_screen(app, "home");
    }
    if !info.handoff {
        if let Some((w, h)) = md(|m| (m.win.clone_strong(), m.hwnd)) {
            let _ = w.show();
            if let Some(h) = h {
                unsafe {
                    let _ = SetForegroundWindow(h);
                }
            }
        }
    }
}

// ---------- Controllers ----------

fn mapping_for(c: &Controller) -> Box<dyn Fn(&Pad) -> Mapping> {
    let pads = c.pads.clone();
    Box::new(move |p: &Pad| pads.iter().find(|(id, _)| *id == p.id).map_or_else(default_mapping, |(_, m)| m.clone()))
}

/// The map of the pad being set up (the first connected), or the default.
fn active_mapping(m: &Media) -> Mapping {
    m.controller.mapping_for(&active_pad(m).unwrap_or_default()).cloned().unwrap_or_else(default_mapping)
}

fn active_pad(m: &Media) -> Option<String> {
    m.pad_ids.first().cloned()
}

fn start_pad_timer() {
    md(|m| {
        m.pad_timer.start(TimerMode::Repeated, Duration::from_millis(16), || {
            super::with(pad_frame);
        });
    });
}

fn pad_frame(app: &Rc<App>) {
    let Some(actions) = md(|m| {
        let front = m.hwnd.is_some_and(|h| unsafe { GetForegroundWindow() } == h);
        if !front {
            m.front = false;
            return Vec::new();
        }
        let pads = m.pads.read();
        let ids: Vec<String> = pads.iter().flatten().map(|p| p.id.clone()).collect();
        let changed = ids != m.pad_ids;
        m.pad_ids = ids;
        if changed {
            m.actions_pending.push("@pads".into());
        }
        // Coming back to Media: buttons still held (the A that closed a game) don't fire.
        if !m.front {
            m.front = true;
            m.reader.reset(&pads);
            return std::mem::take(&mut m.actions_pending);
        }
        if let Some(c) = m.capture.as_mut() {
            for p in pads.iter().flatten() {
                let now = pad::held(p);
                let before = c.held.insert(p.id.clone(), now.clone()).unwrap_or_else(|| now.clone());
                if let Some(b) = now.iter().find(|b| !before.contains(b)) {
                    m.actions_pending.push(format!("@capture:{b}:{}", p.id));
                    break;
                }
            }
            return std::mem::take(&mut m.actions_pending);
        }
        let mut out = std::mem::take(&mut m.actions_pending);
        out.extend(m.reader.update(&pads, now_ms() as f64));
        out
    }) else {
        return;
    };
    for a in actions {
        if a == "@pads" {
            render_prompts();
            settings::pads_changed(app);
        } else if let Some(rest) = a.strip_prefix("@capture:") {
            let (b, id) = rest.split_once(':').unwrap_or(("0", ""));
            settings::captured(app, b.parse().unwrap_or(0), id);
        } else {
            set_input(true);
            on_pad_action(app, &a);
        }
    }
}
