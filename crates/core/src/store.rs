//! apps.json: the list of entries (apps, islands, pins, positions), its
//! backup, and what happens when it can't be read.
use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::layout::{self, Rect};
use crate::safe_file::{corrupt_copy_path, write_file_atomic};
use crate::{Error, Result};

/// One item of apps.json, kept as the JSON object it is on disk rather than
/// as typed fields: 1.2.2 loads hand-edited files with missing, null or
/// wrongly typed values and writes back every key it was given, in the order
/// it found them, and this has to do the same.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Entry(pub Map<String, Value>);

impl Entry {
    pub fn new() -> Self {
        Self::default()
    }

    /// None unless `value` is a JSON object.
    pub fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Object(map) => Some(Entry(map)),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// A new key goes last, an existing one keeps its place, like a JS
    /// property assignment.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.0.insert(key.to_owned(), value.into());
    }

    /// Stores a JS number the way JSON.stringify writes it: `3`, not `3.0`.
    pub fn set_num(&mut self, key: &str, n: f64) {
        self.set(key, js_number(n));
    }

    /// JS `delete`: the other keys keep their order.
    pub fn remove(&mut self, key: &str) {
        self.0.shift_remove(key);
    }

    /// The value when it is a string (`typeof x === 'string'`).
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    /// The value when it is a number (`typeof x === 'number'`).
    pub fn num(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    /// JS truthiness of the value; a missing key is falsy.
    pub fn truthy(&self, key: &str) -> bool {
        truthy(self.get(key))
    }
}

pub(crate) use crate::js::{read_text, truthy};
use crate::js::js_num as js_number;

/// JS `===` between two values read from JSON, `None` standing for
/// undefined. Two arrays or objects are never the same object here.
pub(crate) fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) | (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        _ => false,
    }
}

/// `entry.parentId || null`, with `None` for null (Home).
pub(crate) fn parent_key(entry: &Entry) -> Option<&Value> {
    entry.get("parentId").filter(|v| truthy(Some(v)))
}

pub fn apps_file(dir: &Path) -> PathBuf {
    dir.join("apps.json")
}

fn backup_file(dir: &Path) -> PathBuf {
    dir.join("apps.json.bak")
}

pub fn icons_dir(dir: &Path) -> PathBuf {
    dir.join("icons")
}

pub fn files_dir(dir: &Path) -> PathBuf {
    dir.join("files")
}

pub const READ_ONLY_MESSAGE: &str = "Launchpad can't save changes because apps.json couldn't be read, and saving now would overwrite it. Fix or replace that file (or import a backup from Settings), then try again.";

fn backfill(entry: &mut Entry, key: &str, value: impl Into<Value>) {
    if entry.get(key).is_none() {
        entry.set(key, value);
    }
}

/// Fills in what older versions didn't save. `dir` is the data folder, for
/// stored files' targets.
pub fn normalize(dir: &Path, entry: &mut Entry) {
    // Entries saved before folders/pinning/etc existed are missing these fields.
    if !entry.truthy("kind") {
        entry.set("kind", "app");
    }
    if entry.str("kind") == Some("folder") {
        entry.set("kind", "island"); // one-time rename migration
    }
    backfill(entry, "parentId", Value::Null);
    backfill(entry, "pinned", false);
    backfill(entry, "launchCount", 0);
    backfill(entry, "color", Value::Null);
    backfill(entry, "iconIsCustom", false);
    backfill(entry, "collapsed", true);
    if entry.str("kind") == Some("island") {
        backfill(entry, "w", js_number(layout::EXPANDED_ISLAND_W));
        backfill(entry, "h", js_number(layout::EXPANDED_ISLAND_H));
        backfill(entry, "showNames", false);
    }
    if entry.str("type") == Some("stored") {
        normalize_stored(dir, entry);
    }
}

// `name === path.basename(name)` with Node's Windows rules: no separator of
// either kind, and no drive prefix ("C:name" has the basename "name").
fn is_bare_name(name: &str) -> bool {
    let b = name.as_bytes();
    !name.contains(['/', '\\']) && !(b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

fn path_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

// A stored file's target and thumbnail always live under files\<id>\ (see
// file_store). They're re-derived here rather than trusted from disk, so a
// hand-edited apps.json can't point the launcher at arbitrary paths. A name
// with any path parts in it gets no target at all.
fn normalize_stored(dir: &Path, entry: &mut Entry) {
    let name = entry.str("storedName").unwrap_or("").to_owned();
    let safe_name = !name.is_empty() && is_bare_name(&name) && name != "." && name != "..";
    // Looser than ids::is_uuid on purpose: this is the check 1.2.2 makes.
    let safe_id = entry
        .str("id")
        .filter(|id| id.len() == 36 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'))
        .map(str::to_owned);
    let files = files_dir(dir);
    let target = match &safe_id {
        Some(id) if safe_name => path_string(files.join(id).join(&name)),
        _ => String::new(),
    };
    let icon_path = match &safe_id {
        Some(id) => Value::String(path_string(files.join("thumbs").join(format!("{id}.png")))),
        None => Value::Null,
    };
    entry.set("target", target);
    entry.set("iconPath", icon_path);
    entry.set("iconIsCustom", false);
    if !matches!(entry.str("storedKind"), Some("file" | "folder")) {
        entry.set("storedKind", "file");
    }
    if !matches!(entry.str("thumbState"), Some("ready" | "generic" | "pending")) {
        entry.set("thumbState", "pending");
    }
}

fn cell_of(entry: &Entry) -> Option<(f64, f64)> {
    entry.num("gridX").zip(entry.num("gridY"))
}

/// Assigns a free grid cell to any entry that doesn't have one yet - either
/// because it predates the freeform canvas, or because it just moved to a
/// new parent. Every parent (Home/root, or any given island) is its own
/// independent coordinate space, so this groups siblings and places each
/// group separately. Mutates `apps` in place, returns whether anything changed.
pub fn assign_missing_positions(apps: &mut [Entry]) -> bool {
    let mut dirty = false;
    let mut by_parent: Vec<Vec<usize>> = Vec::new();
    for i in 0..apps.len() {
        let key = parent_key(&apps[i]);
        match by_parent.iter_mut().find(|group| strict_eq(parent_key(&apps[group[0]]), key)) {
            Some(group) => group.push(i),
            None => by_parent.push(vec![i]),
        }
    }

    for mut siblings in by_parent {
        // `createdAt || 0`; the sort is stable, as Array.prototype.sort is.
        let created_at = |i: &usize| apps[*i].num("createdAt").unwrap_or(0.0);
        siblings.sort_by(|a, b| created_at(a).partial_cmp(&created_at(b)).unwrap_or(Ordering::Equal));
        // Every placed sibling counts before anything new is placed. Collecting
        // them along the way (in createdAt order) let an older item that just
        // moved here land on a newer one's cell.
        // Two items already saved on one cell (left by an older version) are
        // pulled apart too: the newer one moves to the nearest free cell.
        let mut occupied: Vec<Rect> = Vec::new();
        for &i in &siblings {
            let Some((x, y)) = cell_of(&apps[i]) else { continue };
            let fp = layout::footprint_of(&apps[i]);
            let mut rect = Rect { x, y, w: fp.w, h: fp.h };
            if occupied.iter().any(|o| layout::rects_overlap(&rect, o)) {
                let pos = layout::find_nearest_free_cell(&occupied, x, y, fp.w, fp.h);
                apps[i].set_num("gridX", pos.x);
                apps[i].set_num("gridY", pos.y);
                rect.x = pos.x;
                rect.y = pos.y;
                dirty = true;
            }
            occupied.push(rect);
        }
        for &i in &siblings {
            if cell_of(&apps[i]).is_some() {
                continue;
            }
            let fp = layout::footprint_of(&apps[i]);
            let pos = layout::find_first_free_cell(&occupied, fp.w, fp.h, layout::DEFAULT_MAX_COLS);
            apps[i].set_num("gridX", pos.x);
            apps[i].set_num("gridY", pos.y);
            occupied.push(Rect { x: pos.x, y: pos.y, w: fp.w, h: fp.h });
            dirty = true;
        }
    }
    dirty
}

// shortcut: String.prototype.localeCompare is ICU collation, which needs a
// dependency; this is case-insensitive code point order with lowercase first.
// Only decides the one-time pin order of same-ranked items with non-ASCII or
// punctuation-led names. Use real collation if the UI ever sorts by name.
fn locale_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

// Gives every currently-pinned item a stable taskbar order the first time
// this runs (existing items keep whatever order they already had; anything
// unordered gets appended after, alphabetically). Mutates in place.
fn assign_missing_pin_orders(apps: &mut [Entry]) -> bool {
    let mut pinned: Vec<usize> = (0..apps.len()).filter(|&i| apps[i].truthy("pinned")).collect();
    if pinned.iter().all(|&i| apps[i].num("pinOrder").is_some()) {
        return false;
    }
    let order = |i: usize| apps[i].num("pinOrder").unwrap_or(f64::INFINITY);
    // A pinned entry whose name isn't a string made 1.2.2 throw here; it sorts as "".
    let name = |i: usize| apps[i].str("name").unwrap_or("");
    pinned.sort_by(|&a, &b| match order(a).partial_cmp(&order(b)) {
        Some(Ordering::Equal) | None => locale_compare(name(a), name(b)),
        Some(unequal) => unequal,
    });
    for (n, i) in pinned.into_iter().enumerate() {
        apps[i].set("pinOrder", n);
    }
    true
}

pub fn next_pin_order(apps: &[Entry]) -> f64 {
    apps.iter()
        .filter(|a| a.truthy("pinned"))
        .filter_map(|a| a.num("pinOrder"))
        .reduce(f64::max)
        .map_or(0.0, |max| max + 1.0)
}

enum ReadError {
    /// Missing, locked, permission denied.
    Io(io::Error),
    /// Not JSON, or not an array: "corrupt", as opposed to "busy".
    Unparseable(String),
}

// Reads and parses an apps list.
fn read_apps_file(dir: &Path, file: &Path) -> std::result::Result<Vec<Entry>, ReadError> {
    let raw = read_text(file).map_err(ReadError::Io)?;
    let base = file.file_name().unwrap_or_default().to_string_lossy();
    let parsed: Value =
        serde_json::from_str(&raw).map_err(|e| ReadError::Unparseable(format!("{base}: {e}")))?;
    let Value::Array(items) = parsed else {
        return Err(ReadError::Unparseable(format!("{base} is not a JSON array")));
    };
    Ok(items
        .into_iter()
        .filter_map(Entry::from_value)
        .map(|mut entry| {
            normalize(dir, &mut entry);
            entry
        })
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadIssueState {
    /// Rolled back to apps.json.bak.
    Restored,
    /// No usable backup either, so `save` refuses rather than replacing the
    /// user's data with an empty list.
    ReadOnly,
}

/// What happened to an apps.json that wouldn't parse. The unreadable file
/// was copied to `corrupt_copy` first either way. Serializes to the object
/// 1.2.2 sent its renderer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadIssue {
    pub state: LoadIssueState,
    pub corrupt_copy: PathBuf,
    pub apps_file: PathBuf,
}

/// apps.json in one data folder, with the state the JS kept in module
/// variables: whether it is read-only, and what the last load ran into.
pub struct Store {
    dir: PathBuf,
    load_issue: Option<(LoadIssueState, PathBuf)>,
    read_only: bool,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Store { dir: dir.into(), load_issue: None, read_only: false }
    }

    pub fn data_dir(&self) -> &Path {
        &self.dir
    }

    pub fn apps_file(&self) -> PathBuf {
        apps_file(&self.dir)
    }

    pub fn icons_dir(&self) -> PathBuf {
        icons_dir(&self.dir)
    }

    pub fn files_dir(&self) -> PathBuf {
        files_dir(&self.dir)
    }

    fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.icons_dir())
    }

    // apps.json exists but won't parse. Keep a copy of it, then roll back to
    // apps.json.bak if that's readable, otherwise go read-only.
    fn recover_from_unreadable(&mut self, why: &str) -> Result<Vec<Entry>> {
        if self.read_only {
            return Ok(Vec::new()); // already copied aside and reported this session
        }

        let corrupt_copy = corrupt_copy_path(&self.apps_file());
        fs::copy(self.apps_file(), &corrupt_copy)?;
        eprintln!("apps.json is unreadable ({why}); kept a copy at {}", corrupt_copy.display());

        if let Ok(restored) = read_apps_file(&self.dir, &backup_file(&self.dir)) {
            write_file_atomic(&self.apps_file(), serde_json::to_string_pretty(&restored)?)?;
            self.load_issue = Some((LoadIssueState::Restored, corrupt_copy));
            return Ok(restored);
        }
        self.read_only = true;
        self.load_issue = Some((LoadIssueState::ReadOnly, corrupt_copy));
        Ok(Vec::new())
    }

    /// A missing apps.json is an empty list (first run). One that is locked
    /// or permission-denied is an `Error::Io`. One that won't parse is
    /// restored from the backup or leaves the store read-only: see
    /// `get_load_issue`.
    pub fn load(&mut self) -> Result<Vec<Entry>> {
        self.ensure_dirs()?;
        let mut apps = match read_apps_file(&self.dir, &self.apps_file()) {
            Ok(apps) => apps,
            Err(ReadError::Unparseable(why)) => {
                let apps = self.recover_from_unreadable(&why)?;
                if self.read_only {
                    return Ok(apps); // nothing to backfill, and save() would refuse
                }
                apps
            }
            Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::NotFound => Vec::new(), // first run - nothing saved yet
            // Locked or permission-denied: fail this call and leave the file
            // alone. Treating it as empty is what used to wipe it on the next save.
            Err(ReadError::Io(e)) => return Err(e.into()),
        };
        if self.read_only {
            // apps.json was fixed, replaced or removed while we were running.
            self.read_only = false;
            self.load_issue = None;
        }

        let mut dirty = assign_missing_positions(&mut apps);
        if assign_missing_pin_orders(&mut apps) {
            dirty = true;
        }
        if dirty {
            self.save(&apps)?;
        }
        Ok(apps)
    }

    // Copies the current apps.json to apps.json.bak before it gets replaced, so
    // a file that later goes bad can be rolled back on the next load. A file
    // that no longer parses is never copied - that would destroy the last good
    // backup. Failing to back up shouldn't block the save itself.
    fn backup_current(&self) {
        if read_apps_file(&self.dir, &self.apps_file()).is_err() {
            return;
        }
        if let Err(e) = fs::copy(self.apps_file(), backup_file(&self.dir)) {
            eprintln!("Failed to back up apps.json: {e}");
        }
    }

    fn write(&self, apps: &[Entry]) -> Result<()> {
        self.ensure_dirs()?;
        self.backup_current();
        write_file_atomic(&self.apps_file(), serde_json::to_string_pretty(apps)?)?;
        Ok(())
    }

    pub fn save(&self, apps: &[Entry]) -> Result<()> {
        if self.read_only {
            return Err(Error::msg(READ_ONLY_MESSAGE));
        }
        self.write(apps)
    }

    /// Deliberate whole-list replacement (backup import). Allowed even in
    /// read-only mode: the user is explicitly choosing new data, and the
    /// unreadable apps.json was already copied aside.
    pub fn replace_all(&mut self, apps: &[Entry]) -> Result<()> {
        self.write(apps)?;
        self.read_only = false;
        self.load_issue = None;
        Ok(())
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// For the UI to tell the user, once at startup, what happened.
    pub fn get_load_issue(&self) -> Option<LoadIssue> {
        let (state, corrupt_copy) = self.load_issue.clone()?;
        Some(LoadIssue { state, corrupt_copy, apps_file: self.apps_file() })
    }
}

pub fn new_id() -> String {
    crate::ids::new_uuid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{list_dir, temp_dir, TempDir};
    use serde_json::json;

    fn entry_with(name: &str, extra: Value) -> Entry {
        let mut e = Entry::from_value(json!({
            "id": format!("00000000-0000-4000-8000-{:012}", name.len()),
            "kind": "app",
            "name": name,
            "createdAt": 1,
        }))
        .unwrap();
        for (key, value) in extra.as_object().unwrap() {
            e.set(key, value.clone());
        }
        e
    }

    fn entry(name: &str) -> Entry {
        entry_with(name, json!({}))
    }

    struct Setup {
        dir: TempDir,
        store: Store,
        file: PathBuf,
        bak: PathBuf,
    }

    fn setup() -> Setup {
        let dir = temp_dir();
        let store = Store::new(dir.path());
        let file = dir.path().join("apps.json");
        let bak = dir.path().join("apps.json.bak");
        Setup { dir, store, file, bak }
    }

    fn corrupt_copies(dir: &Path) -> Vec<String> {
        list_dir(dir).into_iter().filter(|f| f.starts_with("apps.corrupt-")).collect()
    }

    fn read(file: &Path) -> String {
        fs::read_to_string(file).unwrap()
    }

    fn read_json(file: &Path) -> Value {
        serde_json::from_str(&read(file)).unwrap()
    }

    fn names(apps: &[Entry]) -> Vec<&str> {
        apps.iter().map(|a| a.str("name").unwrap()).collect()
    }

    fn state(store: &Store) -> Option<LoadIssueState> {
        store.get_load_issue().map(|issue| issue.state)
    }

    #[test]
    fn missing_apps_json_loads_as_empty_and_can_be_saved() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        assert_eq!(store.load().unwrap(), []);
        store.save(&[entry("A")]).unwrap();
        assert_eq!(read_json(&file)[0]["name"], "A");
        assert_eq!(store.get_load_issue(), None);
    }

    #[test]
    fn save_keeps_the_previous_version_as_apps_json_bak_and_leaves_no_temp_file() {
        let Setup { dir, store, bak, .. } = setup();
        store.save(&[entry("A")]).unwrap();
        store.save(&[entry("A"), entry("BB")]).unwrap();
        assert_eq!(read_json(&bak).as_array().unwrap().len(), 1);
        assert!(!list_dir(dir.path()).iter().any(|f| f.ends_with(".tmp")));
    }

    #[test]
    fn corrupt_apps_json_with_a_good_backup_is_restored_and_the_bad_file_kept() {
        let Setup { dir, mut store, file, bak } = setup();
        fs::write(&bak, serde_json::to_string(&[entry("A")]).unwrap()).unwrap();
        fs::write(&file, r#"[{"id": "trunc"#).unwrap();

        let apps = store.load().unwrap();
        assert_eq!(names(&apps), ["A"]);
        assert_eq!(state(&store), Some(LoadIssueState::Restored));

        let copy = &corrupt_copies(dir.path())[0];
        assert_eq!(read(&dir.path().join(copy)), r#"[{"id": "trunc"#);
        assert_eq!(store.get_load_issue().unwrap().corrupt_copy, dir.path().join(copy));
        // apps.json itself is valid again, and saving works.
        assert_eq!(read_json(&file)[0]["name"], "A");
        store.save(&[entry("A"), entry("BB")]).unwrap();
    }

    #[test]
    fn corrupt_apps_json_with_no_backup_goes_read_only_and_is_never_overwritten() {
        let Setup { dir, mut store, file, .. } = setup();
        fs::write(&file, "not json").unwrap();

        assert_eq!(store.load().unwrap(), []);
        assert_eq!(state(&store), Some(LoadIssueState::ReadOnly));
        match store.save(&[entry("A")]) {
            Err(Error::Msg(text)) => assert!(text.contains("can't save changes"), "{text}"),
            other => panic!("expected the read-only message, got {other:?}"),
        }
        store.load().unwrap();
        store.load().unwrap();
        assert_eq!(read(&file), "not json");
        assert_eq!(corrupt_copies(dir.path()).len(), 1, "only copied aside once");
    }

    #[test]
    fn a_json_value_that_is_not_an_array_counts_as_corrupt() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        fs::write(&file, r#"{"apps": []}"#).unwrap();
        store.load().unwrap();
        assert_eq!(state(&store), Some(LoadIssueState::ReadOnly));
    }

    #[test]
    fn a_corrupt_backup_is_treated_as_no_backup() {
        let Setup { mut store, file, bak, dir: _dir } = setup();
        fs::write(&file, "not json").unwrap();
        fs::write(&bak, "also not json").unwrap();
        store.load().unwrap();
        assert_eq!(state(&store), Some(LoadIssueState::ReadOnly));
    }

    #[test]
    fn read_only_clears_once_apps_json_is_fixed_on_disk() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        fs::write(&file, "not json").unwrap();
        store.load().unwrap();
        fs::write(&file, "[]").unwrap();
        assert_eq!(store.load().unwrap(), []);
        assert_eq!(store.get_load_issue(), None);
        store.save(&[entry("A")]).unwrap();
    }

    #[test]
    fn replace_all_backup_import_works_in_read_only_mode_without_touching_the_old_backup() {
        let Setup { mut store, file, bak, dir: _dir } = setup();
        fs::write(&file, "not json").unwrap();
        fs::write(&bak, "old backup").unwrap();
        store.load().unwrap();

        store.replace_all(&[entry("A")]).unwrap();
        assert_eq!(store.get_load_issue(), None);
        assert_eq!(read_json(&file)[0]["name"], "A");
        assert_eq!(read(&bak), "old backup", "a corrupt file never replaces the backup");
        store.save(&[entry("A"), entry("BB")]).unwrap();
    }

    #[test]
    fn non_object_array_elements_are_dropped_instead_of_crashing() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        fs::write(&file, serde_json::to_string(&json!([null, 5, entry("A")])).unwrap()).unwrap();
        assert_eq!(names(&store.load().unwrap()), ["A"]);
    }

    #[test]
    fn loads_backfill_positions_for_old_entries() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        fs::write(&file, serde_json::to_string(&[entry("A")]).unwrap()).unwrap();
        let apps = store.load().unwrap();
        let grid_x = apps[0].get("gridX").unwrap();
        assert!(grid_x.is_number());
        assert_eq!(&read_json(&file)[0]["gridX"], grid_x);
    }

    #[test]
    fn an_item_moved_in_without_a_cell_never_lands_on_a_newer_sibling_and_stacked_items_are_pulled_apart() {
        let Setup { mut store, dir: _dir, .. } = setup();
        let id = |n: u32| format!("00000000-0000-4000-8000-{n:012}");
        let list = json!([
            { "id": id(1), "kind": "app", "name": "Old", "createdAt": 1 }, // just moved here: no cell yet
            { "id": id(2), "kind": "app", "name": "New", "createdAt": 2, "gridX": 0, "gridY": 0 },
            { "id": id(3), "kind": "app", "name": "Stacked", "createdAt": 3, "gridX": 0, "gridY": 0 },
        ]);
        let list: Vec<Entry> = serde_json::from_value(list).unwrap();
        store.save(&list).unwrap();
        let apps = store.load().unwrap();
        let cells: Vec<String> = apps.iter().map(|a| format!("{},{}", a.0["gridX"], a.0["gridY"])).collect();
        let distinct: std::collections::HashSet<&String> = cells.iter().collect();
        assert_eq!(distinct.len(), 3, "{}", cells.join(" "));
        assert_eq!(cell_of(&apps[1]), Some((0.0, 0.0))); // the older of the stacked pair stays
    }

    // The rest are not in the JS tests: they pin down what the port had to
    // re-create (JSON.stringify's output, normalize on hand-edited input,
    // Node's path.basename, the pin-order migration).

    #[test]
    fn a_load_and_save_writes_the_file_the_js_build_writes_unknown_keys_included() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        // As 1.2.2 saved it: JSON.stringify(apps, null, 2), nothing to backfill.
        let on_disk = r#"[
  {
    "id": "00000000-0000-4000-8000-000000000001",
    "futureKey": {
      "nested": [
        1,
        2.5,
        "é"
      ],
      "empty": []
    },
    "kind": "app",
    "name": "A \"quoted\" name",
    "target": "C:\\Windows\\explorer.exe",
    "parentId": null,
    "pinned": false,
    "createdAt": 1791883800123,
    "launchCount": 0,
    "color": null,
    "iconIsCustom": false,
    "gridX": 2,
    "gridY": 0,
    "collapsed": true
  }
]"#;
        fs::write(&file, on_disk).unwrap();
        let apps = store.load().unwrap();
        assert_eq!(read(&file), on_disk, "a load with nothing to fix leaves the file alone");
        store.save(&apps).unwrap();
        assert_eq!(read(&file), on_disk);
    }

    #[test]
    fn normalize_backfills_in_js_key_order_and_tolerates_wrong_types() {
        let dir = Path::new("data");
        let mut old = Entry::from_value(json!({ "name": "Games", "kind": "folder", "pinned": "yes", "w": null })).unwrap();
        normalize(dir, &mut old);
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"name":"Games","kind":"island","pinned":"yes","w":null,"parentId":null,"launchCount":0,"color":null,"iconIsCustom":false,"collapsed":true,"h":2,"showNames":false}"#
        );

        let mut odd = Entry::from_value(json!({ "kind": 0, "parentId": 7, "type": ["stored"] })).unwrap();
        normalize(dir, &mut odd);
        assert_eq!(odd.str("kind"), Some("app"));
        assert_eq!(odd.get("parentId"), Some(&json!(7)));
        assert_eq!(odd.get("target"), None);
    }

    #[test]
    fn normalize_re_derives_a_stored_entry_s_target_and_thumbnail() {
        let dir = Path::new("data");
        let id = "00000000-0000-4000-8000-00000000000A";
        let stored = |id: Value, name: Value| {
            let mut e = Entry::from_value(json!({
                "id": id, "type": "stored", "storedName": name,
                "target": "C:\\Windows\\System32\\cmd.exe", "iconPath": "C:\\x.png", "iconIsCustom": true,
                "storedKind": "shortcut",
            }))
            .unwrap();
            normalize(dir, &mut e);
            e
        };

        let good = stored(json!(id), json!("report.pdf"));
        assert_eq!(good.str("target"), Some(path_string(dir.join("files").join(id).join("report.pdf")).as_str()));
        assert_eq!(good.str("iconPath"), Some(path_string(dir.join("files").join("thumbs").join(format!("{id}.png"))).as_str()));
        assert_eq!(good.get("iconIsCustom"), Some(&json!(false)));
        assert_eq!(good.str("storedKind"), Some("file"));
        assert_eq!(good.str("thumbState"), Some("pending"));

        for name in ["..\\..\\cmd.exe", "a/b", "sub\\", "C:cmd.exe", ".", "..", ""] {
            let e = stored(json!(id), json!(name));
            assert_eq!(e.str("target"), Some(""), "{name}");
            assert!(e.str("iconPath").is_some());
        }
        assert_eq!(stored(json!(id), json!(5)).str("target"), Some(""));

        for bad_id in [json!("../../etc"), json!(5), json!("00000000-0000-4000-8000-00000000000g")] {
            let e = stored(bad_id, json!("report.pdf"));
            assert_eq!(e.str("target"), Some(""));
            assert_eq!(e.get("iconPath"), Some(&Value::Null));
        }
    }

    #[test]
    fn load_gives_pinned_items_an_order_once_ordered_ones_first_then_by_name() {
        let Setup { mut store, file, dir: _dir, .. } = setup();
        let list = [
            entry_with("zeta", json!({ "pinned": true })),
            entry_with("Alpha", json!({ "pinned": true })),
            entry_with("kept", json!({ "pinned": true, "pinOrder": 5 })),
            entry_with("loose", json!({ "pinOrder": 9 })),
        ];
        fs::write(&file, serde_json::to_string(&list).unwrap()).unwrap();
        let apps = store.load().unwrap();
        let orders: Vec<(&str, Option<f64>)> = apps.iter().map(|a| (a.str("name").unwrap(), a.num("pinOrder"))).collect();
        assert_eq!(orders, [("zeta", Some(2.0)), ("Alpha", Some(1.0)), ("kept", Some(0.0)), ("loose", Some(9.0))]);
        assert_eq!(next_pin_order(&apps), 3.0);
        assert_eq!(next_pin_order(&[]), 0.0);
    }
}
