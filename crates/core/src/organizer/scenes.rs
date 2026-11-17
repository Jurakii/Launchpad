//! Scenes: saved window arrangements for the organizer. A scene remembers,
//! for each window that was open when it was saved, the app (by exe path),
//! the window's title (to tell two windows of one app apart), the display it
//! was on, and where on that display's work area it sat, as fractions of the
//! work area, so it comes back in proportion on a different resolution.
//! Maximized windows are remembered as maximized. Scenes live in
//! layouts.json (`layouts.rs`); a scene saved from an island has that
//! island's id, and "Open as scene" on the island applies it.
//!
//! Everything here is pure, so it can be tested without real windows.
use super::{js_num, js_round, js_trim, utf16_len, utf16_prefix, OpenWindow, Rect};
use serde_json::{Map, Value};
use std::collections::HashSet;

pub const MAX_NAME_LENGTH: usize = 40;
const MAX_WINDOWS: usize = 64;
const MAX_TEXT: usize = 1024;

/// Where a window sat, as fractions (0..1) of its display's work area.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FracRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The display a window was on when its scene was saved.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SavedDisplay {
    pub id: f64,
    pub bounds: Rect,
}

/// A connected display, in DIP.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Display {
    pub id: f64,
    pub bounds: Rect,
    pub work_area: Rect,
}

/// One window of a scene. `extra` is the object as it was read, so keys this
/// version doesn't know (nested ones too) go back to disk where they were;
/// the typed fields are written over it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneWindow {
    pub exe: String,
    pub title: String,
    pub display: SavedDisplay,
    pub rect: FracRect,
    pub maximized: bool,
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub island_id: Option<String>,
    /// `Date.now()` when it was saved. Nothing checks it, so a file may lack it.
    pub saved_at: Option<f64>,
    pub windows: Vec<SceneWindow>,
    /// The object as it was read; see `SceneWindow::extra`.
    pub extra: Map<String, Value>,
}

fn numbers<const N: usize>(v: Option<&Value>, keys: [&str; N]) -> Option<[f64; N]> {
    let v = v?;
    let mut out = [0.0; N];
    for (slot, key) in out.iter_mut().zip(keys) {
        *slot = v.get(key)?.as_f64()?;
    }
    Some(out)
}

// Writes `pairs` over `base`: keys already there keep their place (1.2.2
// wrote scenes back as read), new ones go last.
fn overlay<const N: usize>(base: Option<&Value>, pairs: [(&str, Value); N]) -> Value {
    let mut m = base.and_then(Value::as_object).cloned().unwrap_or_default();
    for (k, v) in pairs {
        m.insert(k.to_owned(), v);
    }
    Value::Object(m)
}

impl SceneWindow {
    fn from_value(v: &Value) -> Option<SceneWindow> {
        let obj = v.as_object()?;
        let display = obj.get("display")?;
        let [bx, by, bw, bh] = numbers(display.get("bounds"), ["x", "y", "width", "height"])?;
        let [x, y, w, h] = numbers(obj.get("rect"), ["x", "y", "w", "h"])?;
        Some(SceneWindow {
            exe: obj.get("exe")?.as_str()?.to_owned(),
            title: obj.get("title")?.as_str()?.to_owned(),
            display: SavedDisplay { id: display.get("id")?.as_f64()?, bounds: Rect::new(bx, by, bw, bh) },
            rect: FracRect { x, y, w, h },
            maximized: obj.get("maximized")?.as_bool()?,
            extra: obj.clone(),
        })
    }

    fn to_value(&self) -> Value {
        let (b, r) = (self.display.bounds, self.rect);
        let old = Value::Object(self.extra.clone());
        let old_display = old.get("display");
        let bounds = overlay(
            old_display.and_then(|d| d.get("bounds")),
            [("x", js_num(b.x)), ("y", js_num(b.y)), ("width", js_num(b.width)), ("height", js_num(b.height))],
        );
        overlay(
            Some(&old),
            [
                ("exe", self.exe.as_str().into()),
                ("title", self.title.as_str().into()),
                ("display", overlay(old_display, [("id", js_num(self.display.id)), ("bounds", bounds)])),
                ("rect", overlay(old.get("rect"), [("x", js_num(r.x)), ("y", js_num(r.y)), ("w", js_num(r.w)), ("h", js_num(r.h))])),
                ("maximized", self.maximized.into()),
            ],
        )
    }
}

impl Scene {
    /// A scene that passed `validate_scene`; None for anything else.
    pub fn from_value(v: &Value) -> Option<Scene> {
        let obj = v.as_object()?;
        Some(Scene {
            id: obj.get("id")?.as_str()?.to_owned(),
            name: obj.get("name")?.as_str()?.to_owned(),
            island_id: obj.get("islandId").and_then(Value::as_str).map(str::to_owned),
            saved_at: obj.get("savedAt").and_then(Value::as_f64),
            windows: obj.get("windows")?.as_array()?.iter().map(SceneWindow::from_value).collect::<Option<_>>()?,
            extra: obj.clone(),
        })
    }

    pub fn to_value(&self) -> Value {
        let mut v = overlay(
            Some(&Value::Object(self.extra.clone())),
            [
                ("id", self.id.as_str().into()),
                ("name", self.name.as_str().into()),
                ("islandId", self.island_id.as_deref().map_or(Value::Null, Value::from)),
            ],
        );
        let m = v.as_object_mut().expect("overlay returns an object");
        if let Some(saved_at) = self.saved_at {
            m.insert("savedAt".into(), js_num(saved_at));
        }
        m.insert("windows".into(), self.windows.iter().map(SceneWindow::to_value).collect());
        v
    }
}

pub fn clean_name(name: &str) -> String {
    utf16_prefix(js_trim(name), MAX_NAME_LENGTH).to_owned()
}

/// Why a scene can't be used, or None if it can. Takes JSON because that is
/// where wrong types come from.
pub fn validate_scene(s: &Value) -> Option<String> {
    fn text(v: Option<&Value>, max: usize) -> Option<&str> {
        v.and_then(Value::as_str).filter(|t| utf16_len(t) <= max)
    }

    if !s.is_object() && !s.is_array() {
        return Some("it is not a scene".into());
    }
    if text(s.get("id"), 64).is_none_or(str::is_empty) {
        return Some("it has no id".into());
    }
    if s.get("name").and_then(Value::as_str).is_none_or(|n| clean_name(n).is_empty()) {
        return Some("it has no name".into());
    }
    if !matches!(s.get("islandId"), Some(Value::Null | Value::String(_))) {
        return Some("its island is not an id".into());
    }
    let windows = match s.get("windows").and_then(Value::as_array) {
        Some(windows) if !windows.is_empty() => windows,
        _ => return Some("it has no windows".into()),
    };
    if windows.len() > MAX_WINDOWS {
        return Some(format!("it has more than {MAX_WINDOWS} windows"));
    }
    for w in windows {
        if text(w.get("exe"), MAX_TEXT).is_none_or(str::is_empty) {
            return Some("a window has no app".into());
        }
        if text(w.get("title"), MAX_TEXT).is_none() {
            return Some("a window has no title".into());
        }
        let display = w.get("display");
        let id = display.and_then(|d| d.get("id")).and_then(Value::as_f64);
        let bounds = numbers(display.and_then(|d| d.get("bounds")), ["x", "y", "width", "height"]);
        if id.is_none() || bounds.is_none() {
            return Some("a window has no display".into());
        }
        let Some([x, y, width, h]) = numbers(w.get("rect"), ["x", "y", "w", "h"]) else {
            return Some("a window has no position".into());
        };
        if x < 0.0 || y < 0.0 || width <= 0.0 || h <= 0.0 || x + width > 1.0001 || y + h > 1.0001 {
            return Some("a window is off its display".into());
        }
        if !w.get("maximized").is_some_and(Value::is_boolean) {
            return Some("a window has no maximized flag".into());
        }
    }
    None
}

/// The scenes worth keeping from a file: valid ones, first of each id.
pub fn normalize_scenes(list: Option<&Value>) -> Vec<Scene> {
    let mut seen = HashSet::new();
    let list = list.and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .filter_map(|s| {
            if let Some(problem) = validate_scene(s) {
                eprintln!("Skipping a scene in layouts.json: {problem}");
                return None;
            }
            let scene = Scene::from_value(s)?;
            seen.insert(scene.id.clone()).then_some(scene)
        })
        .collect()
}

fn round4(n: f64) -> f64 {
    js_round(n * 10000.0) / 10000.0
}

/// A window's frame (DIP) as fractions of a work area. The part of the window
/// outside the work area is cut off; a window entirely outside it ends up as
/// a thin sliver at the nearest edge rather than being lost.
pub fn to_fractions(frame: Rect, work_area: Rect) -> FracRect {
    let clamp = |v: f64, lo: f64, hi: f64| hi.min(lo.max(v));
    let left = clamp(frame.x, work_area.x, work_area.x + work_area.width - 1.0);
    let top = clamp(frame.y, work_area.y, work_area.y + work_area.height - 1.0);
    let right = clamp(frame.x + frame.width, left + 1.0, work_area.x + work_area.width);
    let bottom = clamp(frame.y + frame.height, top + 1.0, work_area.y + work_area.height);
    FracRect {
        x: round4((left - work_area.x) / work_area.width),
        y: round4((top - work_area.y) / work_area.height),
        w: round4((right - left) / work_area.width),
        h: round4((bottom - top) / work_area.height),
    }
}

/// Back to a DIP rect on a (possibly different) work area.
pub fn from_fractions(rect: FracRect, work_area: Rect) -> Rect {
    let left = js_round(work_area.x + rect.x * work_area.width);
    let top = js_round(work_area.y + rect.y * work_area.height);
    let right = js_round(work_area.x + (rect.x + rect.w) * work_area.width);
    let bottom = js_round(work_area.y + (rect.y + rect.h) * work_area.height);
    Rect { x: left, y: top, width: (right - left).max(1.0), height: (bottom - top).max(1.0) }
}

/// The display a saved window goes back to: the same one if it's still
/// there, else one in the same place with the same size, else the primary.
pub fn pick_display<'a>(saved: &SavedDisplay, displays: &'a [Display], primary: &'a Display) -> &'a Display {
    displays
        .iter()
        .find(|d| d.id == saved.id)
        .or_else(|| displays.iter().find(|d| d.bounds == saved.bounds))
        .unwrap_or(primary)
}

pub fn exe_key(exe: &str) -> String {
    exe.to_lowercase()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Matched<'a> {
    pub entry: &'a SceneWindow,
    pub window: &'a OpenWindow,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Matches<'a> {
    pub matched: Vec<Matched<'a>>,
    pub missing: Vec<&'a SceneWindow>,
}

/// Pairs a scene's windows with open windows (in z-order, top first).
/// Windows of the same app are told apart by title first; whatever is left
/// of that app goes in z-order. Each open window is used once.
pub fn match_windows<'a>(entries: &'a [SceneWindow], windows: &'a [OpenWindow]) -> Matches<'a> {
    let mut free: Vec<&OpenWindow> = windows.iter().collect();
    let mut pairs: Vec<Option<&OpenWindow>> = vec![None; entries.len()];
    for same_title in [true, false] {
        for (entry, pair) in entries.iter().zip(pairs.iter_mut()) {
            if pair.is_some() {
                continue;
            }
            let found = free
                .iter()
                .position(|w| exe_key(&w.exe) == exe_key(&entry.exe) && (!same_title || w.title == entry.title));
            if let Some(j) = found {
                *pair = Some(free.remove(j));
            }
        }
    }
    let mut out = Matches::default();
    for (entry, pair) in entries.iter().zip(pairs) {
        match pair {
            Some(window) => out.matched.push(Matched { entry, window }),
            None => out.missing.push(entry),
        }
    }
    out
}

/// "Scene 1", "Scene 2"…: the first number no scene's name uses yet.
pub fn next_scene_name(scenes: &[Scene]) -> String {
    let mut n = 1;
    loop {
        let name = format!("Scene {n}");
        if !scenes.iter().any(|s| s.name == name) {
            return name;
        }
        n += 1;
    }
}

/// What `app_exe` and `app_name` need of one of the user's items.
// Stands in for the store's item type; merge after Phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SceneApp<'a> {
    pub kind: &'a str,
    /// The item's `type` field.
    pub type_: &'a str,
    pub target: &'a str,
    pub name: &'a str,
}

fn ends_with_exe(s: &str) -> bool {
    let cut = s.len().wrapping_sub(4);
    s.is_char_boundary(cut) && s[cut..].eq_ignore_ascii_case(".exe")
}

/// The exe an island's app runs as, for matching it to windows: its target
/// when that's an .exe (shortcuts are resolved to their target when added).
/// None for websites, scripts, commands and folders, which can't be told
/// apart by their windows.
pub fn app_exe<'a>(app: &SceneApp<'a>) -> Option<&'a str> {
    (app.kind == "app" && app.type_ == "path" && ends_with_exe(app.target)).then_some(app.target)
}

/// A name for an app in a message: Launchpad's own name for it when it's
/// one of the user's apps, else the exe's file name without ".exe",
/// capitalized ("firefox.exe" -> "Firefox").
pub fn app_name(exe: &str, apps: &[SceneApp]) -> String {
    if let Some(app) = apps.iter().find(|a| app_exe(a).is_some_and(|e| exe_key(e) == exe_key(exe))) {
        return app.name.to_owned();
    }
    let mut file = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    if ends_with_exe(file) {
        file = &file[..file.len() - 4];
    }
    if file.is_empty() {
        file = exe;
    }
    let mut chars = file.chars();
    match chars.next() {
        // JS upper-cases one UTF-16 unit, which leaves letters outside the BMP alone.
        Some(first) if (first as u32) < 0x10000 => first.to_uppercase().chain(chars).collect(),
        _ => file.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WORK: Rect = Rect::new(0.0, 40.0, 2000.0, 1000.0);

    fn display(id: f64, x: f64) -> Display {
        Display { id, bounds: Rect::new(x, 0.0, 2000.0, 1040.0), work_area: Rect::new(x, 40.0, 2000.0, 1000.0) }
    }
    fn entry(exe: &str, title: &str) -> Value {
        json!({
            "exe": exe,
            "title": title,
            "display": { "id": 1, "bounds": { "x": 0, "y": 0, "width": 2000, "height": 1040 } },
            "rect": { "x": 0, "y": 0, "w": 0.5, "h": 1 },
            "maximized": false,
        })
    }
    fn typed(exe: &str, title: &str) -> SceneWindow {
        SceneWindow::from_value(&entry(exe, title)).unwrap()
    }
    fn scene_with(windows: Value) -> Value {
        json!({ "id": "s1", "name": "Coding", "islandId": null, "savedAt": 1, "windows": windows })
    }
    fn scene() -> Value {
        scene_with(json!([entry("C:\\Apps\\Code.exe", "main.js - Launchpad")]))
    }
    fn with(mut v: Value, key: &str, value: Value) -> Value {
        v[key] = value;
        v
    }
    fn window(hwnd: i64, exe: &str, title: &str) -> OpenWindow {
        OpenWindow { hwnd, exe: exe.into(), title: title.into(), minimized: false }
    }

    #[test]
    fn validate_scene_accepts_a_saved_scene_and_names_what_is_wrong_with_a_broken_one() {
        let problem = |v: Value| validate_scene(&v).unwrap_or_default();
        let e = || entry("a.exe", "t");
        assert_eq!(validate_scene(&scene()), None);
        assert_eq!(validate_scene(&with(scene(), "islandId", json!("island-1"))), None);
        assert!(problem(with(scene(), "id", json!(""))).contains("id"));
        assert!(problem(with(scene(), "name", json!("   "))).contains("name"));
        assert!(problem(with(scene(), "islandId", json!(7))).contains("island"));
        assert!(problem(scene_with(json!([]))).contains("no windows"));
        assert!(problem(scene_with(json!([with(e(), "exe", json!(""))]))).contains("app"));
        let off = json!({ "x": 0.6, "y": 0, "w": 0.5, "h": 1 });
        assert!(problem(scene_with(json!([with(e(), "rect", off)]))).contains("off its display"));
        assert!(problem(scene_with(json!([with(e(), "display", json!({ "id": "x" }))]))).contains("display"));
        assert!(problem(scene_with(json!([with(e(), "maximized", json!("yes"))]))).contains("maximized"));
    }

    #[test]
    fn normalize_scenes_keeps_valid_scenes_first_of_each_id() {
        let list = json!([scene(), with(scene(), "name", json!("Duplicate")), { "id": "broken" }, null]);
        let kept = normalize_scenes(Some(&list));
        assert_eq!(kept.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Coding"]);
        assert_eq!(normalize_scenes(None), []);
    }

    #[test]
    fn to_fractions_and_from_fractions_round_trip_a_window_and_scale_to_another_work_area() {
        let frame = Rect::new(500.0, 290.0, 1000.0, 500.0);
        let rect = to_fractions(frame, WORK);
        assert_eq!(rect, FracRect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
        assert_eq!(from_fractions(rect, WORK), frame);
        assert_eq!(
            from_fractions(rect, Rect::new(-1920.0, 0.0, 1920.0, 1080.0)),
            Rect::new(-1440.0, 270.0, 960.0, 540.0)
        );
    }

    #[test]
    fn to_fractions_cuts_off_the_part_of_a_window_outside_the_work_area() {
        assert_eq!(
            to_fractions(Rect::new(-100.0, 0.0, 600.0, 2000.0), WORK),
            FracRect { x: 0.0, y: 0.0, w: 0.25, h: 1.0 }
        );
        // Entirely off to the right: a sliver at the right edge, still valid.
        let off = to_fractions(Rect::new(2500.0, 100.0, 300.0, 300.0), WORK);
        assert!(off.x + off.w <= 1.0 && off.w > 0.0);
    }

    #[test]
    fn pick_display_prefers_the_same_display_then_one_with_the_same_bounds_then_the_primary() {
        let displays = [display(1.0, 0.0), display(2.0, 2000.0)];
        let (a, b) = (&displays[0], &displays[1]);
        let primary = a;
        let pick = |id: f64, bounds: Rect| pick_display(&SavedDisplay { id, bounds }, &displays, primary);
        assert!(std::ptr::eq(pick(2.0, b.bounds), b));
        assert!(std::ptr::eq(pick(99.0, b.bounds), b));
        assert!(std::ptr::eq(pick(99.0, Rect::new(9.0, 9.0, 9.0, 9.0)), primary));
    }

    #[test]
    fn match_windows_tells_windows_of_one_app_apart_by_title_then_takes_the_rest_in_z_order() {
        let entries = [
            typed("C:\\FF\\firefox.exe", "Docs"),
            typed("C:\\FF\\firefox.exe", "Mail"),
            typed("C:\\Apps\\Spotify.exe", "Spotify"),
        ];
        let windows = [
            window(1, "c:\\ff\\FIREFOX.EXE", "News"), // top of the z-order
            window(2, "C:\\FF\\firefox.exe", "Mail"),
            window(3, "C:\\Apps\\Code.exe", "Code"),
        ];
        let m = match_windows(&entries, &windows);
        assert_eq!(
            m.matched.iter().map(|m| (m.entry.title.as_str(), m.window.hwnd)).collect::<Vec<_>>(),
            [
                ("Docs", 1), // no title match, so the top Firefox window left
                ("Mail", 2), // same title
            ]
        );
        assert_eq!(m.missing.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Spotify"]);
    }

    #[test]
    fn match_windows_uses_each_open_window_once() {
        let entries = [typed("a.exe", "One"), typed("a.exe", "Two")];
        let windows = [window(5, "a.exe", "Two")];
        let m = match_windows(&entries, &windows);
        assert_eq!(m.matched.iter().map(|m| m.entry.title.as_str()).collect::<Vec<_>>(), ["Two"]);
        assert_eq!(m.missing.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["One"]);
    }

    #[test]
    fn next_scene_name_takes_the_first_free_number() {
        let named = |name: &str| Scene { name: name.into(), ..Default::default() };
        assert_eq!(next_scene_name(&[]), "Scene 1");
        assert_eq!(next_scene_name(&[named("Scene 1"), named("Scene 3")]), "Scene 2");
    }

    #[test]
    fn app_exe_is_an_apps_exe_target_app_name_prefers_the_users_name_for_it() {
        let code = SceneApp { kind: "app", type_: "path", target: "C:\\Apps\\Code.exe", name: "VS Code" };
        assert_eq!(app_exe(&code), Some("C:\\Apps\\Code.exe"));
        assert_eq!(app_exe(&SceneApp { type_: "url", target: "https://example.com", ..code }), None);
        assert_eq!(app_exe(&SceneApp { target: "C:\\Projects", ..code }), None);
        assert_eq!(app_exe(&SceneApp { kind: "island", ..Default::default() }), None);
        assert_eq!(app_name("c:\\apps\\code.exe", &[code]), "VS Code");
        assert_eq!(app_name("C:\\Other\\Spotify.exe", &[code]), "Spotify");
    }

    // Not in the JS tests: what the port added.

    #[test]
    fn from_fractions_rounds_halves_up_left_of_the_primary() {
        // -1001 + 0.5 * 1001 = -500.5, which Math.round sends to -500.
        let r = from_fractions(FracRect { x: 0.5, y: 0.0, w: 0.5, h: 1.0 }, Rect::new(-1001.0, 0.0, 1001.0, 500.0));
        assert_eq!(r, Rect::new(-500.0, 0.0, 500.0, 500.0));
    }

    #[test]
    fn scenes_round_trip_through_json_with_their_unknown_keys() {
        let mut v = scene();
        v["colour"] = json!("red");
        v["windows"][0]["monitorName"] = json!("DELL");
        let s = Scene::from_value(&v).unwrap();
        assert_eq!((s.saved_at, s.island_id.as_deref(), s.windows[0].rect.w), (Some(1.0), None, 0.5));
        assert_eq!(s.to_value(), v);
        assert_eq!(serde_json::to_string(&s.to_value()).unwrap(), serde_json::to_string(&v).unwrap());
    }

    #[test]
    fn app_name_falls_back_to_the_whole_path_and_names_are_cut_at_40() {
        assert_eq!(app_name("firefox.exe", &[]), "Firefox");
        assert_eq!(app_name("C:/x/.exe", &[]), "C:/x/.exe");
        assert_eq!(clean_name(&format!("  {}  ", "n".repeat(50))).len(), 40);
    }
}
