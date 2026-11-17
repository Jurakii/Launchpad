//! Window layouts for the organizer, kept in `<data folder>/layouts.json`
//! next to apps.json. The built-in layouts always exist and are never written
//! to the file, so a later version can adjust them; the file only holds the
//! user's own layouts (made in the Layouts editor), which layouts are hidden,
//! organizer preferences such as the gap, and the user's scenes (saved
//! window arrangements, see `scenes.rs`). A missing file just means
//! "built-ins and defaults".
use super::scenes::{self, Scene};
use super::whole_number;
use super::zones::{validate_layout, Layout, Zone};
use crate::safe_file::{corrupt_copy_path, write_file_atomic};
use crate::{Error, Result};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub const DEFAULT_GAP: i32 = 8;
const MAX_GAP: i32 = 64;
const FILE_NAME: &str = "layouts.json";
const BUILT_IN_IDS: [&str; 5] = ["halves", "thirds", "focus", "main-stack", "quarters"];

pub fn built_in_layouts() -> Vec<Layout> {
    let layout = |id: &str, name: &str, cols, rows, zones: &[Zone]| Layout {
        id: id.into(),
        name: name.into(),
        cols,
        rows,
        zones: zones.to_vec(),
        extra: Map::new(),
    };
    let zone = Zone::new;
    vec![
        layout("halves", "Halves", 2, 1, &[zone("left", "Left", 0, 0, 1, 1), zone("right", "Right", 1, 0, 1, 1)]),
        layout(
            "thirds",
            "Thirds",
            3,
            1,
            &[zone("left", "Left", 0, 0, 1, 1), zone("center", "Center", 1, 0, 1, 1), zone("right", "Right", 2, 0, 1, 1)],
        ),
        layout("focus", "Focus ⅔", 3, 1, &[zone("main", "Main", 0, 0, 2, 1), zone("side", "Side", 2, 0, 1, 1)]),
        layout(
            "main-stack",
            "Main + stack",
            5,
            2,
            &[
                zone("main", "Main", 0, 0, 3, 2),
                zone("top", "Top right", 3, 0, 2, 1),
                zone("bottom", "Bottom right", 3, 1, 2, 1),
            ],
        ),
        layout(
            "quarters",
            "Quarters",
            2,
            2,
            &[
                zone("top-left", "Top left", 0, 0, 1, 1),
                zone("top-right", "Top right", 1, 0, 1, 1),
                zone("bottom-left", "Bottom left", 0, 1, 1, 1),
                zone("bottom-right", "Bottom right", 1, 1, 1, 1),
            ],
        ),
    ]
}

fn is_built_in(id: &str) -> bool {
    BUILT_IN_IDS.contains(&id)
}

/// What `load` returns: the built-ins first, then the user's layouts.
#[derive(Debug, Clone, PartialEq)]
pub struct Data {
    pub gap: i32,
    pub hidden: Vec<String>,
    pub layouts: Vec<Layout>,
    pub scenes: Vec<Scene>,
    /// The file's top-level object as it was read. `save` writes its unknown
    /// keys back where they were, so a save from this version doesn't drop
    /// data a newer version wrote; the keys above are written over it.
    pub raw: Map<String, Value>,
}

/// A layout marked for the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryLayout {
    pub layout: Layout,
    pub built_in: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    pub layouts: Vec<LibraryLayout>,
    pub gap: i32,
}

fn layouts_file(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

// Same rules as settings.json: missing is normal, an unparseable file is
// moved aside once (so the next save starts clean instead of overwriting
// it), and any other read error fails so save() never builds on {}.
fn read(dir: &Path) -> Result<Map<String, Value>> {
    let file = layouts_file(dir);
    let bytes = match fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e.into()),
    };
    // Lossy, as Node reads it: bad UTF-8 inside a string doesn't make the
    // whole file unreadable.
    if let Ok(Value::Object(parsed)) = serde_json::from_str(&String::from_utf8_lossy(&bytes)) {
        return Ok(parsed);
    }

    let corrupt_copy = corrupt_copy_path(&file);
    fs::rename(&file, &corrupt_copy)
        .map_err(|e| Error::msg(format!("layouts.json is unreadable and could not be moved aside: {e}")))?;
    eprintln!("layouts.json is unreadable; moved it to {}", corrupt_copy.display());
    Ok(Map::new())
}

// Built-ins first, then the user's valid layouts. Unknown keys are kept so
// a save from this version doesn't drop data a newer version wrote.
fn normalize(data: Map<String, Value>) -> Data {
    let mut seen: HashSet<String> = BUILT_IN_IDS.iter().map(|id| (*id).to_owned()).collect();
    let mut layouts = built_in_layouts();
    for l in data.get("layouts").and_then(Value::as_array).into_iter().flatten() {
        // a built-in's id, or one already used
        if l.get("id").and_then(Value::as_str).is_some_and(|id| seen.contains(id)) {
            continue;
        }
        if let Some(problem) = validate_layout(l) {
            eprintln!("Skipping a layout in layouts.json: {problem}");
        } else if let Some(layout) = Layout::from_value(l) {
            seen.insert(layout.id.clone());
            layouts.push(layout);
        }
    }
    let gap = whole_number(data.get("gap"))
        .filter(|g| *g >= 0.0 && *g <= f64::from(MAX_GAP))
        .map_or(DEFAULT_GAP, |g| g as i32);
    let mut hidden: Vec<String> = Vec::new();
    for id in data.get("hidden").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        if seen.contains(id) && !hidden.iter().any(|h| h == id) {
            hidden.push(id.to_owned());
        }
    }
    let scenes = scenes::normalize_scenes(data.get("scenes"));
    Data { gap, hidden, layouts, scenes, raw: data }
}

impl Data {
    // The object the JS kept in memory ({ ...data, version: 1, gap, hidden,
    // layouts, scenes }): keys already in the file stay where they are, new
    // ones go at the end in that order.
    fn to_map(&self, with_built_ins: bool) -> Map<String, Value> {
        let mut m = self.raw.clone();
        m.insert("version".into(), 1.into());
        m.insert("gap".into(), self.gap.into());
        m.insert("hidden".into(), self.hidden.iter().map(String::as_str).collect());
        let layouts = self.layouts.iter().filter(|l| with_built_ins || !is_built_in(&l.id));
        m.insert("layouts".into(), layouts.map(Layout::to_value).collect());
        m.insert("scenes".into(), self.scenes.iter().map(Scene::to_value).collect());
        m
    }
}

pub fn load(dir: &Path) -> Data {
    // An error means locked right now - use defaults for this read only.
    normalize(read(dir).unwrap_or_default())
}

// What goes in the file: everything but the built-ins.
fn write(dir: &Path, normalized: Data) -> Result<Data> {
    let text = serde_json::to_string_pretty(&Value::Object(normalized.to_map(false)))?;
    write_file_atomic(&layouts_file(dir), text)?;
    Ok(normalized)
}

/// Writes `data` after checking it like a file that was read: invalid
/// layouts and scenes are dropped. Returns what was kept.
pub fn save(dir: &Path, data: &Data) -> Result<Data> {
    write(dir, normalize(data.to_map(true)))
}

/// The layouts the panel, the drag strip and the taskbar's "Snap to" menu
/// offer: all but the hidden ones. Should every one be hidden (a hand-edited
/// file), all of them, so there's always something to snap to.
pub fn visible(data: &Data) -> Vec<&Layout> {
    let shown: Vec<&Layout> = data.layouts.iter().filter(|l| !data.hidden.contains(&l.id)).collect();
    if shown.is_empty() {
        data.layouts.iter().collect()
    } else {
        shown
    }
}

/// Every layout, marked for the editor, plus the gap.
pub fn library(data: &Data) -> Library {
    let layouts = data
        .layouts
        .iter()
        .map(|l| LibraryLayout { layout: l.clone(), built_in: is_built_in(&l.id), hidden: data.hidden.contains(&l.id) })
        .collect();
    Library { layouts, gap: data.gap }
}

// For the editor's changes: unlike load(), a file that can't be read right
// now fails instead of reading as empty, so saving one layout can never
// wipe out the others.
fn update(dir: &Path, change: impl FnOnce(&mut Data) -> Result<()>) -> Result<Data> {
    let mut data = normalize(read(dir)?);
    change(&mut data)?;
    save(dir, &data)
}

/// Adds the layout, or replaces the one with its id. Fails with a message
/// ready for the user if it isn't one that can be saved.
pub fn save_layout(dir: &Path, layout: &Layout) -> Result<Data> {
    if is_built_in(&layout.id) {
        return Err(Error::msg("Built-in layouts can't be changed. Duplicate it to make your own."));
    }
    let clean = Layout {
        id: layout.id.clone(),
        name: scenes::clean_name(&layout.name),
        cols: layout.cols,
        rows: layout.rows,
        zones: layout.zones.iter().map(|z| Zone::new(&z.id, &scenes::clean_name(&z.name), z.x, z.y, z.w, z.h)).collect(),
        extra: Map::new(),
    };
    if let Some(problem) = validate_layout(&clean.to_value()) {
        return Err(Error::msg(format!("That layout can't be saved: {problem}.")));
    }
    update(dir, |data| {
        match data.layouts.iter_mut().find(|l| l.id == clean.id) {
            Some(existing) => *existing = clean,
            None => data.layouts.push(clean),
        }
        Ok(())
    })
}

pub fn remove_layout(dir: &Path, id: &str) -> Result<Data> {
    if is_built_in(id) {
        return Err(Error::msg("Built-in layouts can't be deleted, only hidden."));
    }
    update(dir, |data| {
        data.layouts.retain(|l| l.id != id);
        Ok(())
    })
}

pub fn set_hidden(dir: &Path, id: &str, hide: bool) -> Result<Data> {
    update(dir, |data| {
        if !data.layouts.iter().any(|l| l.id == id) {
            return Err(Error::msg("That layout no longer exists."));
        }
        data.hidden.retain(|h| h != id);
        if hide && data.hidden.len() + 1 >= data.layouts.len() {
            return Err(Error::msg("At least one layout has to stay shown."));
        }
        if hide {
            data.hidden.push(id.to_owned());
        }
        Ok(())
    })
}

pub fn set_gap(dir: &Path, gap: i32) -> Result<Data> {
    if !(0..=MAX_GAP).contains(&gap) {
        return Err(Error::msg(format!("The gap has to be a whole number from 0 to {MAX_GAP}.")));
    }
    update(dir, |data| {
        data.gap = gap;
        Ok(())
    })
}

// ---------- scenes ----------

/// Adds the scene, or replaces the one with its id. It's checked here like
/// a scene read from the file.
pub fn save_scene(dir: &Path, scene: &Scene) -> Result<Data> {
    let clean = Scene { name: scenes::clean_name(&scene.name), ..scene.clone() };
    if let Some(problem) = scenes::validate_scene(&clean.to_value()) {
        return Err(Error::msg(format!("That scene can't be saved: {problem}.")));
    }
    update(dir, |data| {
        match data.scenes.iter_mut().find(|s| s.id == clean.id) {
            Some(existing) => *existing = clean,
            None => data.scenes.push(clean),
        }
        Ok(())
    })
}

pub fn rename_scene(dir: &Path, id: &str, name: &str) -> Result<Data> {
    let clean = scenes::clean_name(name);
    if clean.is_empty() {
        return Err(Error::msg("A scene needs a name."));
    }
    update(dir, |data| {
        let scene = data.scenes.iter_mut().find(|s| s.id == id).ok_or_else(|| Error::msg("That scene no longer exists."))?;
        scene.name = clean;
        Ok(())
    })
}

pub fn remove_scene(dir: &Path, id: &str) -> Result<Data> {
    update(dir, |data| {
        data.scenes.retain(|s| s.id != id);
        Ok(())
    })
}

/// For backups: what the file holds (the user's layouts, which ones are
/// hidden, the gap, the scenes). Fails when the file can't be read, so an
/// export never quietly leaves them out.
pub fn export_data(dir: &Path) -> Result<Value> {
    Ok(Value::Object(normalize(read(dir)?).to_map(false)))
}

/// Replaces the file with a backup's layouts (anything invalid is dropped).
pub fn import_data(dir: &Path, data: &Value) -> Result<Data> {
    write(dir, normalize(data.as_object().cloned().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{list_dir, temp_dir, TempDir};
    use serde_json::json;

    // The JS tests also counted console.error calls; these go to stderr
    // unchecked, there being nothing here to intercept them with.

    fn setup(file_contents: Option<Value>) -> TempDir {
        let dir = temp_dir();
        if let Some(contents) = file_contents {
            fs::write(layouts_file(dir.path()), contents.to_string()).unwrap();
        }
        dir
    }
    fn on_disk(dir: &TempDir) -> Value {
        serde_json::from_str(&fs::read_to_string(layouts_file(dir.path())).unwrap()).unwrap()
    }

    fn custom_named(id: &str, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "cols": 4,
            "rows": 1,
            "zones": [
                { "id": "left", "x": 0, "y": 0, "w": 1, "h": 1 },
                { "id": "center", "x": 1, "y": 0, "w": 2, "h": 1 },
                { "id": "right", "x": 3, "y": 0, "w": 1, "h": 1 },
            ],
        })
    }
    fn custom(id: &str) -> Value {
        custom_named(id, "Wide center")
    }
    fn typed(v: Value) -> Layout {
        Layout::from_value(&v).unwrap()
    }
    fn with(mut v: Value, key: &str, value: Value) -> Value {
        v[key] = value;
        v
    }
    fn ids(layouts: &[Layout]) -> Vec<&str> {
        layouts.iter().map(|l| l.id.as_str()).collect()
    }
    fn json_ids(list: &Value) -> Vec<&str> {
        list.as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect()
    }
    fn built_ins_and(more: &[&'static str]) -> Vec<&'static str> {
        [&BUILT_IN_IDS[..], more].concat()
    }
    fn message<T: std::fmt::Debug>(r: Result<T>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn with_no_file_the_built_ins_in_order_and_the_default_gap_without_creating_the_file() {
        let dir = setup(None);
        let data = load(dir.path());
        assert_eq!(ids(&data.layouts), BUILT_IN_IDS);
        assert_eq!(data.gap, DEFAULT_GAP);
        assert_eq!(list_dir(dir.path()), [] as [&str; 0]);
    }

    #[test]
    fn the_users_layouts_follow_the_built_ins_invalid_ones_and_built_in_overrides_are_skipped() {
        let broken = with(custom("broken"), "zones", json!([{ "id": "a", "x": 0, "y": 0, "w": 9, "h": 1 }]));
        let overriding = custom_named("halves", "Not really halves");
        let dir = setup(Some(json!({ "layouts": [custom("wide"), broken, overriding] })));

        let data = load(dir.path());
        assert_eq!(ids(&data.layouts), built_ins_and(&["wide"]));
        assert_eq!(data.layouts.iter().find(|l| l.id == "halves").unwrap().name, "Halves");
    }

    #[test]
    fn the_gap_falls_back_to_the_default_unless_it_is_a_whole_number_from_0_to_64() {
        let cases = [
            (json!(0), 0),
            (json!(12), 12),
            (json!(64), 64),
            (json!(65), 8),
            (json!(-1), 8),
            (json!(2.5), 8),
            (json!("8"), 8),
        ];
        for (gap, expected) in cases {
            let dir = setup(Some(json!({ "gap": gap })));
            assert_eq!(load(dir.path()).gap, expected, "gap {gap}");
        }
    }

    #[test]
    fn a_corrupt_layouts_json_is_moved_aside_and_the_defaults_are_used() {
        let dir = setup(None);
        fs::write(layouts_file(dir.path()), "{\"layouts\": [").unwrap();
        assert_eq!(ids(&load(dir.path()).layouts), BUILT_IN_IDS);
        let copies: Vec<String> = list_dir(dir.path()).into_iter().filter(|f| f.starts_with("layouts.corrupt-")).collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read_to_string(dir.path().join(&copies[0])).unwrap(), "{\"layouts\": [");
    }

    #[test]
    fn save_writes_only_the_user_layouts_keeps_unknown_keys_and_round_trips() {
        let dir = setup(Some(json!({ "activeByDisplay": { "123": "thirds" } })));
        let mut data = load(dir.path());
        data.layouts.push(typed(custom("wide")));
        data.gap = 0;
        save(dir.path(), &data).unwrap();

        let written = on_disk(&dir);
        assert_eq!(json_ids(&written["layouts"]), ["wide"]);
        assert_eq!(written["activeByDisplay"], json!({ "123": "thirds" }));

        let reloaded = load(dir.path());
        assert_eq!(ids(&reloaded.layouts), built_ins_and(&["wide"]));
        assert_eq!(reloaded.gap, 0);
    }

    #[test]
    fn load_hands_out_copies_so_changing_one_does_not_change_the_built_ins() {
        let dir = setup(None);
        load(dir.path()).layouts[0].zones[0].w = 99;
        assert_eq!(load(dir.path()).layouts[0].zones[0].w, 1);
    }

    #[test]
    fn a_second_layout_with_an_id_already_in_use_is_skipped() {
        let dir = setup(Some(json!({ "layouts": [custom("wide"), custom_named("wide", "Another")] })));
        let data = load(dir.path());
        let wide: Vec<&str> = data.layouts.iter().filter(|l| l.id == "wide").map(|l| l.name.as_str()).collect();
        assert_eq!(wide, ["Wide center"]);
    }

    #[test]
    fn save_layout_adds_a_layout_replaces_it_by_id_and_refuses_built_ins_and_broken_ones() {
        let dir = setup(None);
        save_layout(dir.path(), &typed(custom("wide"))).unwrap();
        save_layout(dir.path(), &typed(custom_named("wide", "  Renamed  "))).unwrap();
        let data = load(dir.path());
        assert_eq!(ids(&data.layouts), built_ins_and(&["wide"]));
        assert_eq!(data.layouts.last().unwrap().name, "Renamed");
        assert_eq!(json_ids(&on_disk(&dir)["layouts"]), ["wide"]);

        assert!(message(save_layout(dir.path(), &typed(custom("halves")))).contains("Built-in"));
        let overlapping = json!([{ "id": "a", "x": 0, "y": 0, "w": 2, "h": 1 }, { "id": "b", "x": 1, "y": 0, "w": 1, "h": 1 }]);
        assert!(message(save_layout(dir.path(), &typed(with(custom("bad"), "zones", overlapping)))).contains("overlap"));
        assert!(message(save_layout(dir.path(), &typed(custom_named("bad", "   ")))).contains("missing name"));
        assert_eq!(load(dir.path()).layouts.len(), BUILT_IN_IDS.len() + 1, "nothing was saved by the failures");
    }

    // The JS made fs.readFileSync throw EBUSY; here the file is really held
    // open without sharing, which only Windows can do.
    #[cfg(windows)]
    #[test]
    fn save_layout_throws_rather_than_overwriting_a_file_it_cannot_read() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = setup(Some(json!({ "layouts": [custom("wide")] })));
        let lock = fs::OpenOptions::new().read(true).share_mode(0).open(layouts_file(dir.path())).unwrap();
        assert!(matches!(save_layout(dir.path(), &typed(custom("other"))), Err(Error::Io(_))));
        // load() is the tolerant one: defaults for this read only.
        assert_eq!(ids(&load(dir.path()).layouts), BUILT_IN_IDS);
        drop(lock);
        assert_eq!(json_ids(&on_disk(&dir)["layouts"]), ["wide"]);
    }

    #[test]
    fn remove_layout_deletes_only_user_layouts_and_forgets_that_they_were_hidden() {
        let dir = setup(Some(json!({ "layouts": [custom("wide")], "hidden": ["wide", "thirds"] })));
        assert!(message(remove_layout(dir.path(), "halves")).contains("only hidden"));
        let data = remove_layout(dir.path(), "wide").unwrap();
        assert_eq!(ids(&data.layouts), BUILT_IN_IDS);
        assert_eq!(data.hidden, ["thirds"]);
    }

    #[test]
    fn hidden_layouts_are_left_out_of_visible_but_one_always_stays_shown() {
        let dir = setup(Some(json!({ "layouts": [custom("wide")] })));
        let dir = dir.path();
        set_hidden(dir, "thirds", true).unwrap();
        set_hidden(dir, "wide", true).unwrap();
        let data = load(dir);
        let shown: Vec<&str> = visible(&data).iter().map(|l| l.id.as_str()).collect();
        assert_eq!(shown, ["halves", "focus", "main-stack", "quarters"]);
        let lib = library(&data);
        let marked = |pick: fn(&LibraryLayout) -> bool| -> Vec<&str> {
            lib.layouts.iter().filter(|l| pick(l)).map(|l| l.layout.id.as_str()).collect()
        };
        assert_eq!(marked(|l| l.hidden), ["thirds", "wide"]);
        assert_eq!(marked(|l| l.built_in), BUILT_IN_IDS);

        set_hidden(dir, "wide", false).unwrap();
        for id in ["halves", "focus", "main-stack", "quarters"] {
            set_hidden(dir, id, true).unwrap();
        }
        assert!(message(set_hidden(dir, "wide", true)).contains("stay shown"));
        assert!(message(set_hidden(dir, "nope", true)).contains("no longer exists"));

        let data = load(dir);
        assert_eq!(visible(&data).iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["wide"]);
        let all_hidden = Data { hidden: data.layouts.iter().map(|l| l.id.clone()).collect(), ..data.clone() };
        assert_eq!(visible(&all_hidden).len(), data.layouts.len(), "all hidden by hand: all shown");
    }

    #[test]
    fn hidden_ids_that_are_not_layouts_are_dropped() {
        let dir = setup(Some(json!({ "hidden": ["thirds", "gone", 42, "thirds"] })));
        assert_eq!(load(dir.path()).hidden, ["thirds"]);
    }

    #[test]
    fn set_gap_accepts_whole_numbers_from_0_to_64() {
        let dir = setup(None);
        assert_eq!(set_gap(dir.path(), 0).unwrap().gap, 0);
        assert_eq!(load(dir.path()).gap, 0);
        // Not expressible with an i32: the JS cases 2.5 and '8'.
        for bad in [-1, 65] {
            assert!(message(set_gap(dir.path(), bad)).contains("whole number"));
        }
    }

    #[test]
    fn export_data_and_import_data_round_trip_the_user_layouts_hidden_list_and_gap() {
        let first = setup(Some(json!({ "layouts": [custom("wide")], "hidden": ["thirds"], "gap": 4 })));
        let backup = export_data(first.path()).unwrap();
        assert_eq!(json_ids(&backup["layouts"]), ["wide"], "no built-ins in the backup");

        let second = setup(Some(json!({ "layouts": [custom("other")] })));
        let mut with_broken = backup.clone();
        with_broken["layouts"].as_array_mut().unwrap().push(json!({ "id": "broken" }));
        let data = import_data(second.path(), &with_broken).unwrap();
        assert_eq!(ids(&data.layouts), built_ins_and(&["wide"]), "replaced, and the broken one dropped");
        assert_eq!(data.hidden, ["thirds"]);
        assert_eq!(data.gap, 4);
        assert_eq!(ids(&import_data(second.path(), &Value::Null).unwrap().layouts), BUILT_IN_IDS);
    }

    fn saved_scene(id: &str, name: &str, island_id: Option<&str>) -> Value {
        json!({
            "id": id,
            "name": name,
            "islandId": island_id,
            "savedAt": 1,
            "windows": [{
                "exe": "C:\\Apps\\Code.exe",
                "title": "main.js",
                "display": { "id": 1, "bounds": { "x": 0, "y": 0, "width": 1920, "height": 1080 } },
                "rect": { "x": 0, "y": 0, "w": 0.5, "h": 1 },
                "maximized": false,
            }],
        })
    }
    fn scene(id: &str) -> Scene {
        Scene::from_value(&saved_scene(id, "Coding", None)).unwrap()
    }
    fn scene_ids(data: &Data) -> Vec<&str> {
        data.scenes.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn scenes_save_scene_adds_and_replaces_by_id_rename_scene_and_remove_scene_change_the_file() {
        let dir = setup(None);
        save_scene(dir.path(), &scene("a")).unwrap();
        save_scene(dir.path(), &Scene::from_value(&saved_scene("b", "Writing", Some("island-1"))).unwrap()).unwrap();
        save_scene(dir.path(), &Scene { name: "  Coding 2  ".into(), ..scene("a") }).unwrap();
        let data = load(dir.path());
        let names: Vec<(&str, &str)> = data.scenes.iter().map(|s| (s.id.as_str(), s.name.as_str())).collect();
        assert_eq!(names, [("a", "Coding 2"), ("b", "Writing")]);

        rename_scene(dir.path(), "b", "Reading").unwrap();
        remove_scene(dir.path(), "a").unwrap();
        let on_disk = on_disk(&dir);
        let rows: Vec<Value> = on_disk["scenes"].as_array().unwrap().iter().map(|s| json!([s["id"], s["name"], s["islandId"]])).collect();
        assert_eq!(rows, [json!(["b", "Reading", "island-1"])]);
    }

    #[test]
    fn scenes_broken_ones_are_refused_when_saved_and_skipped_when_read() {
        let dir = setup(Some(json!({ "scenes": [saved_scene("ok", "Coding", None), { "id": "broken", "windows": [] }] })));
        assert_eq!(scene_ids(&load(dir.path())), ["ok"]);
        let no_windows = Scene { windows: vec![], ..scene("x") };
        assert!(message(save_scene(dir.path(), &no_windows)).contains("can't be saved: it has no windows"));
        assert!(message(rename_scene(dir.path(), "ok", "   ")).contains("needs a name"));
        assert!(message(rename_scene(dir.path(), "gone", "Name")).contains("no longer exists"));
    }

    #[test]
    fn scenes_saving_layouts_keeps_the_scenes_and_backups_carry_them() {
        let first = setup(Some(json!({ "scenes": [saved_scene("a", "Coding", None)] })));
        set_gap(first.path(), 12).unwrap();
        assert_eq!(scene_ids(&load(first.path())), ["a"]);
        let backup = export_data(first.path()).unwrap();

        let second = setup(None);
        assert_eq!(scene_ids(&import_data(second.path(), &backup).unwrap()), ["a"]);
    }

    // Not in the JS tests: the file as text, which both builds have to agree on.

    #[test]
    fn the_file_is_printed_like_json_stringify_with_unknown_keys_where_they_were() {
        let dir = setup(None);
        fs::write(layouts_file(dir.path()), r#"{"future":{"a":[]},"gap":4,"later":1.5}"#).unwrap();
        save_layout(dir.path(), &typed(with(custom("wide"), "cols", json!(4.0)))).unwrap();
        save_scene(dir.path(), &scene("s")).unwrap();
        // node: JSON.stringify(layouts.load() minus built-ins, null, 2) after the same two calls
        let expected = r#"{
  "future": {
    "a": []
  },
  "gap": 4,
  "later": 1.5,
  "version": 1,
  "hidden": [],
  "layouts": [
    {
      "id": "wide",
      "name": "Wide center",
      "cols": 4,
      "rows": 1,
      "zones": [
        {
          "id": "left",
          "name": "",
          "x": 0,
          "y": 0,
          "w": 1,
          "h": 1
        },
        {
          "id": "center",
          "name": "",
          "x": 1,
          "y": 0,
          "w": 2,
          "h": 1
        },
        {
          "id": "right",
          "name": "",
          "x": 3,
          "y": 0,
          "w": 1,
          "h": 1
        }
      ]
    }
  ],
  "scenes": [
    {
      "id": "s",
      "name": "Coding",
      "islandId": null,
      "savedAt": 1,
      "windows": [
        {
          "exe": "C:\\Apps\\Code.exe",
          "title": "main.js",
          "display": {
            "id": 1,
            "bounds": {
              "x": 0,
              "y": 0,
              "width": 1920,
              "height": 1080
            }
          },
          "rect": {
            "x": 0,
            "y": 0,
            "w": 0.5,
            "h": 1
          },
          "maximized": false
        }
      ]
    }
  ]
}"#;
        assert_eq!(fs::read_to_string(layouts_file(dir.path())).unwrap(), expected);
    }

    #[test]
    fn wrong_types_in_a_hand_edited_file_load_as_defaults_instead_of_failing() {
        let dir = setup(Some(json!({
            "layouts": { "not": "a list" },
            "hidden": "thirds",
            "gap": null,
            "scenes": 7,
            "version": "two",
        })));
        let data = load(dir.path());
        assert_eq!((ids(&data.layouts), data.gap), (BUILT_IN_IDS.to_vec(), DEFAULT_GAP));
        assert!(data.hidden.is_empty() && data.scenes.is_empty());
        assert_eq!(set_gap(dir.path(), 3).unwrap().gap, 3);
        assert_eq!(on_disk(&dir), json!({ "layouts": [], "hidden": [], "gap": 3, "scenes": [], "version": 1 }));

        // A top-level array or text is corrupt to both builds.
        fs::write(layouts_file(dir.path()), "[]").unwrap();
        assert_eq!(load(dir.path()).gap, DEFAULT_GAP);
        assert_eq!(list_dir(dir.path()).len(), 1, "moved aside, not left in place");
    }
}
