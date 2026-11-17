//! Media's library: games and videos found in the user's library folders.
//! Kept in its own library.json rather than apps.json - a few thousand ROMs
//! would otherwise each get a Home grid cell, ride along on every apps:list,
//! and be embedded in every backup. Same safety behavior as the item store:
//! atomic writes, a .bak of the previous version, and a copy-aside +
//! restore-or-read-only path when the file won't parse.
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{ends_with_ignore_case, js_trim, truthy, win_path};
use crate::ids::{is_uuid, new_uuid};
use crate::safe_file::{corrupt_copy_path, write_file_atomic};
use crate::{Error, Result};

pub const LIBRARIES: [&str; 2] = ["games", "videos"];
pub const LAUNCH_TYPES: [&str; 5] = ["profile", "path", "url", "script", "command"];
pub const MEDIA_SLOTS: [&str; 5] = ["cover", "banner", "logo", "screenshot", "video"];

const READ_ONLY_MESSAGE: &str = "Launchpad can't save library changes because library.json couldn't be read, and saving now would overwrite it. Fix or replace that file, then try again.";

/// One library entry, kept as the JSON object it is on disk: keys this build
/// doesn't know, their order, and values of an unexpected type all go back
/// to library.json as they came, exactly as the JS build leaves them.
/// `normalize` guarantees the keys the accessors read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Entry(pub Map<String, Value>);

impl Entry {
    fn text(&self, key: &str) -> &str {
        self.0.get(key).and_then(Value::as_str).unwrap_or("")
    }
    pub fn id(&self) -> &str {
        self.text("id")
    }
    /// `games` or `videos`.
    pub fn library(&self) -> &str {
        self.text("library")
    }
    pub fn name(&self) -> &str {
        self.text("name")
    }
    pub fn platform(&self) -> Option<&str> {
        Some(self.text("platform")).filter(|s| !s.is_empty())
    }
    /// The `type` key: one of `LAUNCH_TYPES`.
    pub fn launch_type(&self) -> &str {
        self.text("type")
    }
    pub fn profile(&self) -> Option<&str> {
        Some(self.text("profile")).filter(|s| !s.is_empty())
    }
    pub fn target(&self) -> &str {
        self.text("target")
    }
    pub fn available(&self) -> bool {
        truthy(self.0.get("available"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadState {
    Restored,
    ReadOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadIssue {
    pub state: LoadState,
    pub corrupt_copy: PathBuf,
    pub library_file: PathBuf,
}

// Ids double as folder names under media\, so only plain UUIDs are allowed:
// anything else could point a write outside that folder.
pub fn is_valid_id(id: &str) -> bool {
    is_uuid(id)
}

pub fn new_id() -> String {
    new_uuid()
}

/// Fills in defaults for missing or unrecognized fields. Does not touch ids;
/// `fix_ids` handles those across the whole list.
pub fn normalize(mut entry: Map<String, Value>) -> Entry {
    let one_of = |v: Option<&Value>, list: &[&str]| v.and_then(Value::as_str).is_some_and(|s| list.contains(&s));
    let is = |v: Option<&Value>, check: fn(&Value) -> bool| v.is_some_and(check);

    if !one_of(entry.get("library"), &LIBRARIES) {
        entry.insert("library".into(), "games".into());
    }
    if !is(entry.get("name"), Value::is_string) {
        entry.insert("name".into(), "".into());
    }
    if !entry.contains_key("platform") {
        entry.insert("platform".into(), Value::Null);
    }
    if !one_of(entry.get("type"), &LAUNCH_TYPES) {
        entry.insert("type".into(), "profile".into());
    }
    if !entry.contains_key("profile") {
        entry.insert("profile".into(), Value::Null);
    }
    if !is(entry.get("target"), Value::is_string) {
        entry.insert("target".into(), "".into());
    }
    if !entry.contains_key("available") {
        entry.insert("available".into(), true.into());
    }
    if !is(entry.get("launchCount"), Value::is_number) {
        entry.insert("launchCount".into(), 0.into());
    }
    if !is(entry.get("createdAt"), Value::is_number) {
        entry.insert("createdAt".into(), 0.into());
    }

    let old = match entry.get_mut("media") {
        Some(Value::Object(m)) => std::mem::take(m),
        _ => Map::new(),
    };
    let mut media = Map::new();
    for slot in MEDIA_SLOTS {
        let kept = match old.get(slot) {
            Some(Value::Object(m)) if is(m.get("path"), Value::is_string) => Value::Object(m.clone()),
            _ => Value::Null,
        };
        media.insert(slot.into(), kept);
    }
    entry.insert("media".into(), Value::Object(media));
    if !is(entry.get("meta"), Value::is_object) {
        entry.insert("meta".into(), Value::Object(Map::new()));
    }
    Entry(entry)
}

/// Gives a fresh id to any entry whose id isn't a UUID or repeats an earlier
/// one. Mutates in place, returns whether anything changed.
pub fn fix_ids(entries: &mut [Entry]) -> bool {
    let mut dirty = false;
    let mut seen = HashSet::new();
    for entry in entries {
        let id = entry.0.get("id").and_then(Value::as_str).unwrap_or("");
        if !is_valid_id(id) || seen.contains(&id.to_ascii_lowercase()) {
            entry.0.insert("id".into(), new_id().into());
            dirty = true;
        }
        seen.insert(entry.id().to_ascii_lowercase());
    }
    dirty
}

enum ReadError {
    Io(io::Error),
    Unparseable(String),
}

fn objects(list: Vec<Value>) -> Vec<Entry> {
    list.into_iter()
        .filter_map(|v| match v {
            Value::Object(m) => Some(normalize(m)),
            _ => None,
        })
        .collect()
}

// Filesystem errors are returned as-is; content that isn't a JSON array is
// `Unparseable`, so load() can tell "corrupt" from "busy".
fn read_library_file(file: &Path) -> std::result::Result<Vec<Entry>, ReadError> {
    // Lossy like Node's utf-8 decoding: stray bytes don't make the read fail.
    let raw = fs::read(file).map_err(ReadError::Io)?;
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    match serde_json::from_str::<Value>(&String::from_utf8_lossy(&raw)) {
        Ok(Value::Array(list)) => Ok(objects(list)),
        Ok(_) => Err(ReadError::Unparseable(format!("{name} is not a JSON array"))),
        Err(e) => Err(ReadError::Unparseable(format!("{name}: {e}"))),
    }
}

// JSON.stringify(entries, null, 2)
fn to_json(entries: &[Entry]) -> Result<String> {
    Ok(serde_json::to_string_pretty(entries)?)
}

/// library.json in one data folder, with what the JS kept in module
/// variables: the read-only flag and the last load issue.
#[derive(Debug)]
pub struct Library {
    dir: PathBuf,
    load_issue: Option<(LoadState, PathBuf)>,
    read_only: bool,
}

impl Library {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Library { dir: dir.into(), load_issue: None, read_only: false }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn library_file(&self) -> PathBuf {
        self.dir.join("library.json")
    }

    fn backup_file(&self) -> PathBuf {
        self.dir.join("library.json.bak")
    }

    pub fn media_root(&self) -> PathBuf {
        self.dir.join("media")
    }

    fn recover_from_unreadable(&mut self, message: &str) -> Result<Vec<Entry>> {
        if self.read_only {
            return Ok(Vec::new()); // already copied aside and reported this session
        }
        let file = self.library_file();
        let corrupt_copy = corrupt_copy_path(&file);
        fs::copy(&file, &corrupt_copy)?;
        eprintln!("library.json is unreadable ({message}); kept a copy at {}", corrupt_copy.display());

        if let Ok(mut restored) = read_library_file(&self.backup_file()) {
            fix_ids(&mut restored);
            write_file_atomic(&file, to_json(&restored)?)?;
            self.load_issue = Some((LoadState::Restored, corrupt_copy));
            return Ok(restored);
        }
        self.read_only = true;
        self.load_issue = Some((LoadState::ReadOnly, corrupt_copy));
        Ok(Vec::new())
    }

    pub fn load(&mut self) -> Result<Vec<Entry>> {
        let mut entries = match read_library_file(&self.library_file()) {
            Ok(entries) => entries,
            Err(ReadError::Unparseable(message)) => {
                let entries = self.recover_from_unreadable(&message)?;
                if self.read_only {
                    return Ok(entries);
                }
                entries
            }
            Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::NotFound => Vec::new(), // no library yet
            // Locked or permission-denied: fail this call, never treat it as empty.
            Err(ReadError::Io(e)) => return Err(e.into()),
        };
        if self.read_only {
            // library.json was fixed, replaced or removed while we were running.
            self.read_only = false;
            self.load_issue = None;
        }

        if fix_ids(&mut entries) {
            self.save(&entries)?;
        }
        Ok(entries)
    }

    // Copies the current library.json to .bak before it is replaced. A file
    // that no longer parses is never copied over the last good backup.
    fn backup_current(&self) {
        if read_library_file(&self.library_file()).is_err() {
            return;
        }
        if let Err(e) = fs::copy(self.library_file(), self.backup_file()) {
            eprintln!("Failed to back up library.json: {e}");
        }
    }

    fn write(&self, entries: &[Entry]) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        self.backup_current();
        write_file_atomic(&self.library_file(), to_json(entries)?)?;
        Ok(())
    }

    pub fn save(&self, entries: &[Entry]) -> Result<()> {
        if self.read_only {
            return Err(Error::msg(READ_ONLY_MESSAGE));
        }
        self.write(entries)
    }

    /// Deliberate whole-list replacement (backup import). Allowed in
    /// read-only mode: the unreadable file was already copied aside.
    pub fn replace_all(&mut self, entries: Vec<Value>) -> Result<Vec<Entry>> {
        let mut list = objects(entries);
        fix_ids(&mut list);
        self.write(&list)?;
        self.read_only = false;
        self.load_issue = None;
        Ok(list)
    }

    pub fn get_load_issue(&self) -> Option<LoadIssue> {
        self.load_issue.as_ref().map(|(state, corrupt_copy)| LoadIssue {
            state: *state,
            corrupt_copy: corrupt_copy.clone(),
            library_file: self.library_file(),
        })
    }

    /// The art folder for one entry, or an error for anything but a UUID.
    pub fn media_dir_for(&self, id: &str) -> Result<PathBuf> {
        if !is_valid_id(id) {
            return Err(Error::msg("Invalid library id"));
        }
        Ok(self.media_root().join(id.to_ascii_lowercase()))
    }
}

// String.prototype.slice(0, n): n counts UTF-16 units. A character that
// would be cut in half is dropped whole.
fn slice_utf16(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// A single program added on its own (Media's + -> Single app), as a library
/// entry of type 'path': it runs the .exe itself, with no player or emulator.
/// `request` is `{ target, name?, library?, platform? }` as the page sent it;
/// `exists` checks the file; `platforms` is the set of known platform ids.
/// Fails with a message for the user when the request isn't usable.
pub fn single_app_entry(
    request: &Value,
    exists: impl Fn(&str) -> bool,
    platforms: &HashSet<String>,
    now: i64,
) -> Result<Entry> {
    let field = |key: &str| request.as_object().and_then(|o| o.get(key));
    let target = field("target").and_then(Value::as_str).unwrap_or("");
    if !win_path::is_absolute(target) || !ends_with_ignore_case(target, ".exe") {
        return Err(Error::msg("Choose a program (.exe) by its full path"));
    }
    if !exists(target) {
        return Err(Error::msg("That program isn't there any more"));
    }
    let lib = match field("library") {
        None => "games",
        Some(v) => v.as_str().filter(|s| LIBRARIES.contains(s)).ok_or_else(|| Error::msg("Choose games or videos"))?,
    };
    let platform = match field("platform") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) if s.is_empty() || platforms.contains(s) => s,
        Some(_) => return Err(Error::msg("Unknown platform")),
    };
    let name = field("name").and_then(Value::as_str).map(|n| slice_utf16(js_trim(n), 120)).unwrap_or("");

    let mut entry = Map::new();
    entry.insert("id".into(), new_id().into());
    entry.insert("library".into(), lib.into());
    entry.insert("name".into(), if name.is_empty() { win_path::stem(target) } else { name }.into());
    entry.insert("platform".into(), if lib == "games" && !platform.is_empty() { platform.into() } else { Value::Null });
    entry.insert("type".into(), "path".into());
    entry.insert("profile".into(), Value::Null);
    entry.insert("target".into(), target.into());
    entry.insert("createdAt".into(), now.into());
    Ok(normalize(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::test_entry;
    use crate::test_util::{list_dir, temp_dir, TempDir};
    use serde_json::json;

    const ID_A: &str = "11111111-1111-4111-8111-111111111111";
    const ID_B: &str = "22222222-2222-4222-8222-222222222222";

    fn entry(name: &str, extra: Value) -> Entry {
        let mut e = json!({
            "id": ID_A,
            "library": "games",
            "name": name,
            "platform": "snes",
            "type": "profile",
            "target": format!("C:\\ROMs\\snes\\{name}.sfc"),
        });
        for (k, v) in extra.as_object().unwrap() {
            e[k] = v.clone();
        }
        test_entry(e)
    }

    fn plain(name: &str) -> Entry {
        entry(name, json!({}))
    }

    struct Setup {
        dir: TempDir,
        library: Library,
        file: PathBuf,
        bak: PathBuf,
    }

    fn setup() -> Setup {
        let dir = temp_dir();
        let library = Library::new(dir.path());
        let file = dir.path().join("library.json");
        let bak = dir.path().join("library.json.bak");
        Setup { dir, library, file, bak }
    }

    fn corrupt_copies(dir: &Path) -> Vec<String> {
        list_dir(dir).into_iter().filter(|f| f.starts_with("library.corrupt-")).collect()
    }

    fn on_disk(file: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(Entry::name).collect()
    }

    fn save_error(library: &Library) -> String {
        library.save(&[plain("A")]).unwrap_err().to_string()
    }

    #[test]
    fn missing_library_json_loads_as_empty_and_can_be_saved() {
        let Setup { dir: _dir, mut library, file, .. } = setup();
        assert!(library.load().unwrap().is_empty());
        library.save(&[plain("A")]).unwrap();
        assert_eq!(on_disk(&file)[0]["name"], "A");
        assert_eq!(library.get_load_issue(), None);
    }

    #[test]
    fn save_keeps_the_previous_version_as_library_json_bak_and_leaves_no_temp_file() {
        let Setup { dir, library, bak, .. } = setup();
        library.save(&[plain("A")]).unwrap();
        library.save(&[plain("A"), entry("B", json!({ "id": ID_B }))]).unwrap();
        assert_eq!(on_disk(&bak).as_array().unwrap().len(), 1);
        assert!(!list_dir(dir.path()).iter().any(|f| f.ends_with(".tmp")));
    }

    #[test]
    fn library_json_and_apps_json_are_separate_files() {
        let Setup { dir, library, .. } = setup();
        library.save(&[plain("A")]).unwrap();
        assert!(!list_dir(dir.path()).contains(&"apps.json".to_string()));
    }

    #[test]
    fn corrupt_library_json_with_a_good_backup_is_restored_and_the_bad_file_kept() {
        let Setup { dir, mut library, file, bak } = setup();
        fs::write(&bak, serde_json::to_string(&[plain("A")]).unwrap()).unwrap();
        fs::write(&file, "[{\"id\": \"trunc").unwrap();

        assert_eq!(names(&library.load().unwrap()), ["A"]);
        assert_eq!(library.get_load_issue().unwrap().state, LoadState::Restored);
        let copies = corrupt_copies(dir.path());
        assert_eq!(fs::read_to_string(dir.path().join(&copies[0])).unwrap(), "[{\"id\": \"trunc");
        assert_eq!(on_disk(&file)[0]["name"], "A");
        library.save(&[plain("A")]).unwrap();
    }

    #[test]
    fn corrupt_library_json_with_no_backup_goes_read_only_and_is_never_overwritten() {
        let Setup { dir, mut library, file, .. } = setup();
        fs::write(&file, "not json").unwrap();

        assert!(library.load().unwrap().is_empty());
        assert_eq!(library.get_load_issue().unwrap().state, LoadState::ReadOnly);
        assert!(save_error(&library).contains("can't save library changes"));
        library.load().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "not json");
        assert_eq!(corrupt_copies(dir.path()).len(), 1, "only copied aside once");
    }

    #[test]
    fn a_json_value_that_is_not_an_array_counts_as_corrupt() {
        let Setup { dir: _dir, mut library, file, .. } = setup();
        fs::write(&file, "{\"games\": []}").unwrap();
        library.load().unwrap();
        assert_eq!(library.get_load_issue().unwrap().state, LoadState::ReadOnly);
    }

    #[test]
    fn read_only_clears_once_library_json_is_fixed_on_disk() {
        let Setup { dir: _dir, mut library, file, .. } = setup();
        fs::write(&file, "not json").unwrap();
        library.load().unwrap();
        fs::write(&file, "[]").unwrap();
        assert!(library.load().unwrap().is_empty());
        assert_eq!(library.get_load_issue(), None);
        library.save(&[plain("A")]).unwrap();
    }

    #[test]
    fn replace_all_works_in_read_only_mode_without_touching_the_old_backup() {
        let Setup { dir: _dir, mut library, file, bak } = setup();
        fs::write(&file, "not json").unwrap();
        fs::write(&bak, "old backup").unwrap();
        library.load().unwrap();

        library.replace_all(vec![Value::Object(plain("A").0)]).unwrap();
        assert_eq!(library.get_load_issue(), None);
        assert_eq!(on_disk(&file)[0]["name"], "A");
        assert_eq!(fs::read_to_string(&bak).unwrap(), "old backup");
        library.save(&[plain("A")]).unwrap();
    }

    #[test]
    fn non_object_elements_are_dropped_instead_of_crashing() {
        let Setup { dir: _dir, mut library, file, .. } = setup();
        fs::write(&file, json!([null, 5, [], plain("A")]).to_string()).unwrap();
        assert_eq!(names(&library.load().unwrap()), ["A"]);
    }

    #[test]
    fn bad_and_duplicate_ids_get_fresh_uuids_saved_back_to_disk() {
        let Setup { dir: _dir, mut library, file, .. } = setup();
        let mut none = plain("None");
        none.0.remove("id");
        fs::write(&file, json!([plain("A"), plain("Dup"), entry("Bad", json!({ "id": "..\\..\\evil" })), none]).to_string())
            .unwrap();
        let entries = library.load().unwrap();
        let ids: Vec<&str> = entries.iter().map(Entry::id).collect();
        assert_eq!(ids[0], ID_A, "a valid first id is kept");
        assert!(ids.iter().all(|id| is_valid_id(id)));
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 4);
        let saved = on_disk(&file);
        assert_eq!(saved.as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn normalize_fills_defaults_and_drops_unknown_values() {
        let e = normalize(
            json!({ "id": ID_A, "library": "music", "type": "rocket", "media": { "cover": "x", "banner": { "path": "b.jpg", "source": "user" }, "junk": {} }, "meta": [] })
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(e.library(), "games");
        assert_eq!(e.launch_type(), "profile");
        assert_eq!(e.0["name"], "");
        assert_eq!(e.0["platform"], Value::Null);
        assert_eq!(e.0["profile"], Value::Null);
        assert_eq!(e.0["target"], "");
        assert_eq!(e.0["available"], true);
        assert_eq!(e.0["launchCount"], 0);
        assert_eq!(e.0["media"].as_object().unwrap().keys().collect::<Vec<_>>(), MEDIA_SLOTS);
        assert_eq!(e.0["media"]["cover"], Value::Null);
        assert_eq!(e.0["media"]["banner"], json!({ "path": "b.jpg", "source": "user" }));
        assert_eq!(e.0["meta"], json!({}));
    }

    #[test]
    fn normalize_keeps_valid_values_as_they_are() {
        let e = normalize(
            entry("A", json!({ "library": "videos", "type": "url", "profile": "mpv", "available": false, "launchCount": 3, "createdAt": 9 })).0,
        );
        assert_eq!(e.library(), "videos");
        assert_eq!(e.launch_type(), "url");
        assert_eq!(e.profile(), Some("mpv"));
        assert!(!e.available());
        assert_eq!(e.0["launchCount"], 3);
        assert_eq!(e.0["createdAt"], 9);
    }

    #[test]
    fn media_dir_for_stays_inside_the_media_folder_and_rejects_non_uuid_ids() {
        let Setup { dir: _dir, library, .. } = setup();
        let dir = library.media_dir_for(ID_A).unwrap();
        assert_eq!(dir.parent().unwrap(), library.media_root());
        // The JS case `null` has no &str form.
        for bad in ["..", "..\\x", "C:\\Windows", "", &format!("{ID_A}\\..")] {
            assert!(library.media_dir_for(bad).unwrap_err().to_string().contains("Invalid library id"));
        }
    }

    fn platform_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_app_entry_makes_a_path_entry_that_runs_the_program_itself() {
        let platforms = platform_set(&["snes", "gamecube"]);
        let make = |request: Value| single_app_entry(&request, |_| true, &platforms, 7).unwrap();
        let e = make(json!({ "target": "C:\\Games\\Celeste\\Celeste.exe", "name": " Celeste ", "library": "games", "platform": "" }));
        assert!(is_valid_id(e.id()));
        assert_eq!(
            json!([e.0["type"], e.0["target"], e.0["name"], e.0["library"], e.0["platform"], e.0["profile"], e.0["available"], e.0["createdAt"], e.0["launchCount"]]),
            json!(["path", "C:\\Games\\Celeste\\Celeste.exe", "Celeste", "games", null, null, true, 7, 0])
        );
        assert_eq!(make(json!({ "target": "C:\\Apps\\Kodi.exe", "library": "videos", "platform": "snes" })).0["platform"], Value::Null);
        assert_eq!(make(json!({ "target": "C:\\Apps\\Kodi.exe" })).name(), "Kodi");
    }

    #[test]
    fn single_app_entry_refuses_anything_but_an_existing_absolute_exe_and_unknown_libraries_or_platforms() {
        let platforms = platform_set(&["snes"]);
        let error = |request: Value| {
            single_app_entry(&request, |p| p != "C:\\Gone.exe", &platforms, 0).unwrap_err().to_string()
        };
        for target in [json!("Game.exe"), json!("C:\\Games\\run.bat"), json!("C:\\Games\\run.cmd"), json!("C:\\Games\\rom.sfc"), json!(""), json!(null)] {
            assert!(error(json!({ "target": target })).contains("full path"), "{target}");
        }
        assert!(error(json!({ "target": "C:\\Gone.exe" })).contains("isn't there"));
        assert!(error(json!({ "target": "C:\\A.exe", "library": "music" })).contains("games or videos"));
        assert!(error(json!({ "target": "C:\\A.exe", "platform": "atari" })).contains("Unknown platform"));
    }
}
