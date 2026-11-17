//! Settings → Backup: export and import (main.js `backup:export` and
//! `backup:import` in 1.2.2). The save/open dialogs are the app's; these take
//! the path the user chose.
//!
//! A backup is `{ version: 3, apps, layouts, library, profiles }`: the
//! apps.json entries with their icons embedded, layouts.json (the user's
//! window layouts), and Media's library and profiles without art
//! (`media::backup`). Version 2 has no library or profiles; older backups are
//! just the array of entries. All still import.
//!
//! Stored files are too big for the JSON, so they're copied to a folder
//! beside it named in `storedFilesFolder` (e.g.
//! `launchpad-backup-2026-10-23-files\<id>\`). That's an additive field:
//! builds without the file store see those entries as broken links.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::ids::{is_uuid, new_uuid};
use crate::js::{read_text, truthy};
use crate::media::backup as media_backup;
use crate::media::library::Library;
use crate::organizer::layouts;
use crate::safe_file::write_file_atomic;
use crate::store::{self, Entry, Store};
use crate::{Error, Result};

pub const VERSION: u32 = 3;
pub const INVALID_BACKUP: &str = "That file is not a valid Launchpad backup.";

/// `launchpad-backup-2026-10-23.json` -> `launchpad-backup-2026-10-23-files`.
pub fn stored_files_folder_for(backup_file: &Path) -> String {
    format!("{}-files", backup_file.file_stem().unwrap_or_default().to_string_lossy())
}

/// What an export did, as 1.2.2 sent it to the page.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub path: PathBuf,
    /// The folder holding stored files' copies; None when there are none.
    pub stored_files_dir: Option<PathBuf>,
    pub stored_count: usize,
    /// Names of stored entries whose copy wasn't in the file store.
    pub stored_missing: Vec<String>,
}

/// Writes a version 3 backup of `store`'s data folder to `file`, and copies
/// stored files to the folder beside it. Fails, writing nothing, when
/// apps.json, layouts.json or Media's files can't be read right now.
pub fn export(store: &mut Store, library: &mut Library, file: &Path) -> Result<Exported> {
    let mut apps = store.load()?;
    for a in &mut apps {
        if let Some(icon) = a.str("iconPath").filter(|p| !p.is_empty()).map(PathBuf::from) {
            if icon.exists() {
                let data = fs::read(&icon)?;
                a.set("iconData", base64_encode(&data));
            }
        }
        a.remove("iconPath");
    }
    let mut bundle = Map::new();
    bundle.insert("version".into(), VERSION.into());
    bundle.insert("apps".into(), serde_json::to_value(&apps)?);
    bundle.insert("layouts".into(), layouts::export_data(store.data_dir())?);
    if let Value::Object(media) = media_backup::export_data(library)? {
        bundle.extend(media);
    }

    let stored: Vec<&Entry> = apps.iter().filter(|a| a.str("type") == Some("stored")).collect();
    let mut stored_files_dir = None;
    let mut stored_missing = Vec::new();
    if !stored.is_empty() {
        let folder = stored_files_folder_for(file);
        let dir = file.parent().unwrap_or(Path::new("")).join(&folder);
        bundle.insert("storedFilesFolder".into(), folder.into());
        for a in &stored {
            // 1.2.2 joined any id into the path; one with path parts in it
            // could read or write outside the two folders, so it counts as missing.
            let src = a.str("id").filter(|id| is_plain_name(id)).map(|id| (id, store.files_dir().join(id)));
            let Some((id, src)) = src.filter(|(_, src)| src.exists()) else {
                stored_missing.push(name_of(a));
                continue;
            };
            let dest = dir.join(id);
            remove_path(&dest)?;
            copy_dir(&src, &dest)?;
        }
        stored_files_dir = Some(dir);
    }
    write_file_atomic(file, serde_json::to_string_pretty(&Value::Object(bundle))?)?;
    Ok(Exported { path: file.to_owned(), stored_files_dir, stored_count: stored.len(), stored_missing })
}

/// What an import did. The app shows the errors and missing names; when
/// `layouts_changed` it tells the organizer (1.2.2's
/// `organizer.layoutsChanged()`), and it decorates `apps` for the page.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub apps: Vec<Entry>,
    pub layouts_changed: bool,
    /// The backup's layouts couldn't be written; the apps are imported anyway.
    pub layouts_error: Option<String>,
    /// Media's library or profiles couldn't be written; the rest is imported.
    pub media_error: Option<String>,
    /// Names of stored entries whose copy wasn't beside the backup.
    pub stored_missing: Vec<String>,
}

/// Replaces apps.json (even when the store is read-only), and layouts.json
/// and Media's library and profiles when the backup has them, with the
/// backup at `file`. Fails with `INVALID_BACKUP` when it isn't one.
pub fn import(store: &mut Store, library: &mut Library, file: &Path) -> Result<Imported> {
    let bundle: Option<Value> = read_text(file).ok().and_then(|raw| serde_json::from_str(&raw).ok());
    let app_list = match &bundle {
        Some(Value::Array(list)) => list,
        Some(Value::Object(b)) => match b.get("apps") {
            Some(Value::Array(list)) => list,
            _ => return Err(Error::msg(INVALID_BACKUP)),
        },
        _ => return Err(Error::msg(INVALID_BACKUP)),
    };
    let bundle = bundle.as_ref().expect("matched above");
    let dir = store.data_dir().to_owned();
    // `e && typeof e === 'object'`: a list passes too, and spreads to its indexes.
    let raw: Vec<&Value> = app_list.iter().filter(|e| e.is_object() || e.is_array()).collect();

    // Ids become icon file names (icons\<id>.png), so anything that isn't a
    // plain UUID - or repeats one - gets a fresh id; a crafted backup could
    // otherwise write outside the icons folder. Children follow the new id.
    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut entries: Vec<(Option<Value>, Entry)> = Vec::new();
    for e in &raw {
        let old = e.get("id").and_then(Value::as_str);
        let id = match old {
            Some(old) if is_uuid(old) && !id_map.contains_key(old) => old.to_owned(),
            _ => new_uuid(),
        };
        if let Some(old) = old {
            id_map.entry(old.to_owned()).or_insert_with(|| id.clone());
        }
        let mut entry = Entry(spread(e));
        let icon_data = entry.0.shift_remove("iconData");
        entry.set("id", id);
        store::normalize(&dir, &mut entry);
        entries.push((icon_data, entry));
    }
    // An item whose parent island isn't in the backup would never be shown.
    let islands: HashSet<String> = entries
        .iter()
        .filter(|(_, e)| e.str("kind") == Some("island"))
        .filter_map(|(_, e)| e.str("id").map(str::to_owned))
        .collect();
    for (_, entry) in &mut entries {
        let parent = store::parent_key(entry).and_then(Value::as_str).and_then(|p| id_map.get(p)).filter(|p| islands.contains(*p)).cloned();
        entry.set("parentId", parent.map_or(Value::Null, Value::String));
    }

    // Stored files come back from the folder beside the backup. One that isn't
    // there stays as an entry (shown as missing) rather than being dropped.
    let folder_name = match bundle.get("storedFilesFolder").and_then(Value::as_str) {
        Some(name) if bundle.is_object() && !name.is_empty() => node_basename(name).to_owned(),
        _ => stored_files_folder_for(file),
    };
    let stored_files_dir = file.parent().unwrap_or(Path::new("")).join(folder_name);
    let mut stored_missing = Vec::new();
    for (i, (_, entry)) in entries.iter().enumerate() {
        if entry.str("type") != Some("stored") {
            continue;
        }
        let target = entry.str("target").unwrap_or("");
        if !target.is_empty() && Path::new(target).exists() {
            continue; // already here (same install)
        }
        let id = entry.str("id").unwrap_or_default();
        let src = raw[i].get("id").and_then(Value::as_str).filter(|old| is_uuid(old)).map(|old| stored_files_dir.join(old));
        if let Some(src) = src.filter(|src| src.exists()) {
            let dest = store.files_dir().join(id);
            match remove_path(&dest).and_then(|()| copy_dir(&src, &dest)) {
                Ok(()) => continue,
                Err(e) => eprintln!("Failed to restore stored file {}: {e}", name_of(entry)),
            }
        }
        stored_missing.push(name_of(entry));
    }

    let mut restored = Vec::with_capacity(entries.len());
    for (icon_data, mut entry) in entries {
        let icon = icon_data.as_ref().and_then(Value::as_str).filter(|d| !d.is_empty()).map(base64_decode);
        if entry.str("type") == Some("stored") {
            // Its icon path is fixed (thumbs\<id>.png); regenerate if the backup had none.
            if let Some(bytes) = icon {
                fs::create_dir_all(store.files_dir().join("thumbs"))?;
                // normalize set iconPath from the id, which is a UUID by now.
                let thumb = entry.str("iconPath").map(PathBuf::from).unwrap_or_default();
                fs::write(thumb, bytes)?;
            } else if entry.str("storedKind") == Some("file") {
                entry.set("thumbState", "pending");
            }
        } else if let Some(bytes) = icon {
            let icon_path = store.icons_dir().join(format!("{}.png", entry.str("id").unwrap_or_default()));
            fs::create_dir_all(store.icons_dir())?;
            fs::write(&icon_path, bytes)?;
            entry.set("iconPath", icon_path.to_string_lossy().into_owned());
        } else {
            entry.set("iconPath", Value::Null);
        }
        restored.push(entry);
    }

    store.replace_all(&restored)?;

    // A backup from before layouts were included leaves the current ones alone.
    let mut layouts_changed = false;
    let mut layouts_error = None;
    if bundle.is_object() && truthy(bundle.get("layouts")) {
        match layouts::import_data(&dir, &bundle["layouts"]) {
            Ok(_) => layouts_changed = true,
            Err(e) => layouts_error = Some(e.to_string()),
        }
    }
    // Version 3 also carries Media's library and profiles; older ones leave them alone.
    let media_error = media_backup::import_data(library, bundle).err().map(|e| e.to_string());
    Ok(Imported { apps: restored, layouts_changed, layouts_error, media_error, stored_missing })
}

// `entry.name` as the page would show it in the "missing" list.
fn name_of(entry: &Entry) -> String {
    match entry.get("name") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "undefined".into(),
    }
}

// No separators, no drive, not `.` or `..`: safe to join under a folder.
fn is_plain_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\\', ':']) && s != "." && s != ".."
}

/// `{ ...value }` in JS: an object's keys, a list's or a string's indexes.
pub(crate) fn spread(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        Value::Array(list) => list.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
        // shortcut: JS indexes UTF-16 units, so a character outside the BMP
        // would be two halves there; here it is one. Only a hand-edited file has
        // a string where an entry goes.
        Value::String(s) => s.chars().enumerate().map(|(i, c)| (i.to_string(), Value::String(c.into()))).collect(),
        _ => Map::new(),
    }
}

/// Node's `path.win32.basename`: the part after the last separator, trailing
/// separators and a drive prefix ignored.
fn node_basename(p: &str) -> &str {
    let mut t = p.trim_end_matches(['/', '\\']);
    let b = t.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        t = &t[2..];
    }
    t.rsplit(['/', '\\']).next().unwrap_or("")
}

// `fs.rm(p, { recursive: true, force: true })`.
pub(crate) fn remove_path(p: &Path) -> io::Result<()> {
    let result = match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) => Err(e),
    };
    match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

// A symlink or junction: never followed, so a folder can't loop or pull in
// content from outside itself (the check media::scan makes).
fn is_link(entry: &fs::DirEntry) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        match entry.metadata() {
            Ok(m) if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => return true,
            Ok(_) => {}
            Err(_) => return true,
        }
    }
    entry.file_type().map_or(true, |t| t.is_symlink())
}

// Recursive copy, symlinks and junctions skipped.
// merge with file_store after Phase 1
fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if is_link(&entry) {
            continue;
        }
        let (s, d) = (entry.path(), to.join(entry.file_name()));
        let t = entry.file_type()?;
        if t.is_dir() {
            copy_dir(&s, &d)?;
        } else if t.is_file() {
            fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `buffer.toString('base64')`.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `Buffer.from(text, 'base64')`, as lenient as Node: the URL-safe alphabet
/// too, anything else skipped, stops at the first `=`, a dangling sixth of a
/// byte dropped.
pub(crate) fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        acc = (acc << 6 | u32::from(v)) & 0xff_ffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::players;
    use crate::test_util::{temp_dir, TempDir};
    use serde_json::json;

    const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const ISLAND: &str = "11111111-1111-4111-8111-111111111111";
    const STORED: &str = "22222222-2222-4222-8222-222222222222";
    const MEDIA_ID: &str = "33333333-3333-4333-8333-333333333333";

    struct Data {
        dir: TempDir,
        store: Store,
        library: Library,
    }

    fn data() -> Data {
        let dir = temp_dir();
        let store = Store::new(dir.path());
        let library = Library::new(dir.path());
        Data { dir, store, library }
    }

    fn entries(list: Value) -> Vec<Entry> {
        list.as_array().unwrap().iter().cloned().filter_map(Entry::from_value).collect()
    }

    fn write_backup(dir: &Path, bundle: &Value) -> PathBuf {
        let file = dir.join("backup.json");
        fs::write(&file, serde_json::to_string_pretty(bundle).unwrap()).unwrap();
        file
    }

    fn by_name<'a>(apps: &'a [Entry], name: &str) -> &'a Entry {
        apps.iter().find(|a| a.str("name") == Some(name)).unwrap()
    }

    // A source data folder: an app with an icon, an island, a stored file
    // with a thumbnail, a layout, a library entry and a profile.
    fn source() -> Data {
        let mut d = data();
        let icons = d.store.icons_dir();
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join(format!("{A}.png")), b"icon-a").unwrap();
        let stored_dir = d.store.files_dir().join(STORED);
        fs::create_dir_all(stored_dir.join("sub")).unwrap();
        fs::write(stored_dir.join("notes.txt"), b"hello").unwrap();
        fs::create_dir_all(d.store.files_dir().join("thumbs")).unwrap();
        fs::write(d.store.files_dir().join("thumbs").join(format!("{STORED}.png")), b"thumb").unwrap();
        let icon = icons.join(format!("{A}.png")).to_string_lossy().into_owned();
        d.store
            .save(&entries(json!([
                { "id": ISLAND, "kind": "island", "name": "Tools", "gridX": 0, "gridY": 0 },
                { "id": A, "name": "Alpha", "type": "path", "target": "C:\\a.exe", "iconPath": icon, "parentId": ISLAND, "gridX": 0, "gridY": 0 },
                { "id": B, "name": "Beta", "type": "url", "target": "https://example.com", "iconPath": "C:\\nowhere\\b.png", "gridX": 1, "gridY": 0 },
                { "id": STORED, "name": "notes.txt", "type": "stored", "storedName": "notes.txt", "storedKind": "file", "thumbState": "ready", "gridX": 2, "gridY": 0 },
            ])))
            .unwrap();
        layouts::import_data(d.dir.path(), &json!({ "gap": 4, "layouts": [{ "id": "wide", "name": "Wide", "cols": 2, "rows": 1, "zones": [{ "id": "l", "name": "L", "x": 0, "y": 0, "w": 1, "h": 1 }] }] })).unwrap();
        d.library.replace_all(vec![json!({ "id": MEDIA_ID, "name": "Arrival", "library": "videos" })]).unwrap();
        players::save(d.dir.path(), &json!({ "profiles": [{ "id": "mpv", "name": "mpv", "kind": "player", "exe": "C:\\mpv\\mpv.exe", "args": ["{file}"], "platforms": ["videos"] }], "defaults": { "videos": "mpv" } })).unwrap();
        d
    }

    #[test]
    fn base64_matches_node() {
        for (bytes, text) in [(&b""[..], ""), (b"f", "Zg=="), (b"fo", "Zm8="), (b"foo", "Zm9v"), (b"hello", "aGVsbG8="), (&[251, 255, 191], "+/+/")] {
            assert_eq!(base64_encode(bytes), text);
            assert_eq!(base64_decode(text), bytes);
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)), all);
        // Values from node: [...Buffer.from(s, 'base64')]
        for (text, bytes) in [("aGVsbG8", &b"hello"[..]), ("aGVs bG8=", b"hello"), ("aGVs*bG8=", b"hello"), ("aGVsbG8=aGVsbG8=", b"hello"), ("a", b""), ("ab", &[105]), ("-_-_", &[251, 255, 191]), ("aGV=sbG8", b"he")] {
            assert_eq!(base64_decode(text), bytes, "{text}");
        }
    }

    #[test]
    fn node_basename_matches_path_win32_basename() {
        for (p, base) in [("a-files", "a-files"), ("C:\\x\\b-files", "b-files"), ("../../evil", "evil"), ("dir/", "dir"), ("C:", ""), ("..", "..")] {
            assert_eq!(node_basename(p), base, "{p}");
        }
        assert_eq!(stored_files_folder_for(Path::new("D:\\b\\launchpad-backup-2026-10-23.json")), "launchpad-backup-2026-10-23-files");
    }

    #[test]
    fn export_embeds_icons_carries_layouts_and_media_and_copies_stored_files() {
        let mut s = source();
        let out = temp_dir();
        let file = out.path().join("launchpad-backup.json");
        let res = export(&mut s.store, &mut s.library, &file).unwrap();
        assert_eq!(res.stored_count, 1);
        assert!(res.stored_missing.is_empty());
        let files_dir = out.path().join("launchpad-backup-files");
        assert_eq!(res.stored_files_dir.as_deref(), Some(files_dir.as_path()));
        assert_eq!(fs::read(files_dir.join(STORED).join("notes.txt")).unwrap(), b"hello");
        assert!(files_dir.join(STORED).join("sub").is_dir());

        let bundle: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        let keys: Vec<&str> = bundle.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["version", "apps", "layouts", "library", "profiles", "storedFilesFolder"]);
        assert_eq!(bundle["version"], 3);
        assert_eq!(bundle["storedFilesFolder"], "launchpad-backup-files");
        let apps = entries(bundle["apps"].clone());
        assert!(apps.iter().all(|a| a.get("iconPath").is_none()), "iconPath is stripped");
        assert_eq!(base64_decode(by_name(&apps, "Alpha").str("iconData").unwrap()), b"icon-a");
        assert!(by_name(&apps, "Beta").get("iconData").is_none(), "an icon that isn't there is left out");
        assert_eq!(base64_decode(by_name(&apps, "notes.txt").str("iconData").unwrap()), b"thumb");
        assert_eq!(bundle["layouts"]["layouts"][0]["id"], "wide");
        assert_eq!(bundle["layouts"]["gap"], 4);
        assert_eq!(bundle["library"][0]["id"], MEDIA_ID);
        assert_eq!(bundle["profiles"]["defaults"]["videos"], "mpv");
    }

    #[test]
    fn export_reports_stored_entries_whose_copy_is_gone_and_skips_the_folder_without_any() {
        let mut s = source();
        fs::remove_dir_all(s.store.files_dir().join(STORED)).unwrap();
        let out = temp_dir();
        let res = export(&mut s.store, &mut s.library, &out.path().join("b.json")).unwrap();
        assert_eq!(res.stored_missing, ["notes.txt"]);

        let mut plain = data();
        plain.store.save(&entries(json!([{ "id": A, "name": "Alpha" }]))).unwrap();
        let res = export(&mut plain.store, &mut plain.library, &out.path().join("c.json")).unwrap();
        assert_eq!((res.stored_count, res.stored_files_dir), (0, None));
        let bundle: Value = serde_json::from_str(&fs::read_to_string(out.path().join("c.json")).unwrap()).unwrap();
        assert!(bundle.get("storedFilesFolder").is_none());
        assert!(!out.path().join("c-files").exists());
    }

    #[test]
    fn export_then_import_brings_everything_back_in_another_folder() {
        let mut s = source();
        let out = temp_dir();
        let file = out.path().join("b.json");
        export(&mut s.store, &mut s.library, &file).unwrap();

        let mut t = data();
        let res = import(&mut t.store, &mut t.library, &file).unwrap();
        assert!(res.layouts_changed);
        assert_eq!((res.layouts_error, res.media_error), (None, None));
        assert!(res.stored_missing.is_empty());
        let apps = t.store.load().unwrap();
        assert_eq!(apps.iter().map(|a| a.str("id").unwrap()).collect::<Vec<_>>(), [ISLAND, A, B, STORED], "UUIDs are kept");
        let alpha = by_name(&apps, "Alpha");
        assert_eq!(alpha.str("parentId"), Some(ISLAND));
        let icon = t.store.icons_dir().join(format!("{A}.png"));
        assert_eq!(alpha.str("iconPath"), Some(icon.to_string_lossy().as_ref()), "icons go to icons\\<id>.png");
        assert_eq!(fs::read(icon).unwrap(), b"icon-a");
        assert_eq!(by_name(&apps, "Beta").get("iconPath"), Some(&Value::Null));
        let stored = by_name(&apps, "notes.txt");
        assert_eq!(fs::read(stored.str("target").unwrap()).unwrap(), b"hello");
        assert_eq!(fs::read(stored.str("iconPath").unwrap()).unwrap(), b"thumb");
        assert_eq!(stored.str("thumbState"), Some("ready"));
        assert!(stored.str("target").unwrap().starts_with(&*t.dir.path().to_string_lossy()));
        assert_eq!(layouts::load(t.dir.path()).gap, 4);
        assert_eq!(t.library.load().unwrap()[0].id(), MEDIA_ID);
        assert_eq!(players::load(t.dir.path()).unwrap().profiles[0].id, "mpv");
    }

    #[test]
    fn import_gives_fresh_ids_to_non_uuid_and_repeated_ids_and_children_follow() {
        let mut t = data();
        let file = write_backup(
            t.dir.path(),
            &json!([
                { "id": "../../evil", "kind": "island", "name": "Isle" },
                { "id": "child", "name": "Child", "parentId": "../../evil" },
                { "id": A, "name": "First" },
                { "id": A, "name": "Second" },
                { "name": "No id", "parentId": A },
                { "id": 7, "name": "Number id" },
            ]),
        );
        let apps = import(&mut t.store, &mut t.library, &file).unwrap().apps;
        assert_eq!(apps.len(), 6);
        let ids: Vec<&str> = apps.iter().map(|a| a.str("id").unwrap()).collect();
        assert!(ids.iter().all(|id| is_uuid(id)), "{ids:?}");
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 6, "all distinct");
        assert_ne!(ids[0], "../../evil");
        assert_eq!(apps[1].str("parentId"), Some(ids[0]), "the child follows its island's new id");
        assert_eq!(ids[2], A, "the first of a repeated id keeps it");
        assert_ne!(ids[3], A);
        assert_eq!(apps[4].get("parentId"), Some(&Value::Null), "A is not an island");
        assert_eq!(fs::read_dir(t.store.icons_dir()).unwrap().count(), 0);
    }

    #[test]
    fn import_sends_items_whose_parent_island_is_not_in_the_backup_home() {
        let mut t = data();
        let file = write_backup(
            t.dir.path(),
            &json!({ "version": 2, "apps": [
                { "id": ISLAND, "kind": "folder", "name": "Old folder" },
                { "id": A, "name": "In old folder", "parentId": ISLAND },
                { "id": B, "name": "Orphan", "parentId": STORED },
                { "id": STORED, "name": "Not an island", "parentId": 0 },
            ] }),
        );
        let apps = import(&mut t.store, &mut t.library, &file).unwrap().apps;
        assert_eq!(apps[0].str("kind"), Some("island"), "'folder' is migrated first");
        assert_eq!(apps[1].str("parentId"), Some(ISLAND));
        assert_eq!(apps[2].get("parentId"), Some(&Value::Null));
        assert_eq!(apps[3].get("parentId"), Some(&Value::Null));
    }

    #[test]
    fn import_restores_stored_copies_by_their_original_id_and_keeps_missing_ones() {
        let mut t = data();
        let backup_dir = temp_dir();
        let folder = backup_dir.path().join("custom-files");
        fs::create_dir_all(folder.join(A)).unwrap();
        fs::write(folder.join(A).join("a.txt"), b"a").unwrap();
        fs::create_dir_all(folder.join(B).join("Pics")).unwrap();
        let file = backup_dir.path().join("b.json");
        let stored = |id: &str, name: &str, kind: &str| json!({ "id": id, "name": name, "type": "stored", "storedName": name, "storedKind": kind, "thumbState": "ready" });
        let mut with_icon = stored(A, "a.txt", "file");
        with_icon["iconData"] = base64_encode(b"thumb-a").into();
        let bundle = json!({ "version": 3, "apps": [with_icon, stored(B, "Pics", "folder"), stored(STORED, "gone.txt", "file"), stored("not-a-uuid", "odd.txt", "file")], "storedFilesFolder": "..\\elsewhere\\custom-files" });
        fs::write(&file, bundle.to_string()).unwrap();

        let res = import(&mut t.store, &mut t.library, &file).unwrap();
        assert_eq!(res.stored_missing, ["gone.txt", "odd.txt"]);
        let a = &res.apps[0];
        assert_eq!(fs::read(a.str("target").unwrap()).unwrap(), b"a");
        assert_eq!(fs::read(a.str("iconPath").unwrap()).unwrap(), b"thumb-a", "the embedded icon is the thumbnail");
        assert_eq!(a.str("thumbState"), Some("ready"));
        assert!(Path::new(res.apps[1].str("target").unwrap()).is_dir());
        assert_eq!(res.apps[1].str("thumbState"), Some("ready"), "folders have no thumbnail to make");
        assert_eq!(res.apps[2].str("thumbState"), Some("pending"), "a file without one gets it regenerated");
        assert_eq!(res.apps.len(), 4, "missing copies stay as entries");

        // Imported again on the same install: the copies are already there.
        fs::remove_dir_all(&folder).unwrap();
        let res = import(&mut t.store, &mut t.library, &file).unwrap();
        assert_eq!(res.stored_missing, ["gone.txt", "odd.txt"]);
    }

    #[test]
    fn import_finds_stored_copies_by_the_backup_name_when_the_folder_is_not_named() {
        let mut t = data();
        let backup_dir = temp_dir();
        fs::create_dir_all(backup_dir.path().join("old-files").join(A)).unwrap();
        fs::write(backup_dir.path().join("old-files").join(A).join("x.txt"), b"x").unwrap();
        let file = backup_dir.path().join("old.json");
        fs::write(&file, json!({ "version": 3, "apps": [{ "id": A, "name": "x.txt", "type": "stored", "storedName": "x.txt" }] }).to_string()).unwrap();
        let res = import(&mut t.store, &mut t.library, &file).unwrap();
        assert!(res.stored_missing.is_empty());
        assert_eq!(fs::read(res.apps[0].str("target").unwrap()).unwrap(), b"x");
    }

    #[test]
    fn version_2_leaves_media_alone_and_a_bare_array_leaves_layouts_alone_too() {
        let mut t = source();
        let v2 = write_backup(t.dir.path(), &json!({ "version": 2, "apps": [{ "id": A, "name": "Only" }], "layouts": { "gap": 12 } }));
        let res = import(&mut t.store, &mut t.library, &v2).unwrap();
        assert!(res.layouts_changed);
        assert_eq!(layouts::load(t.dir.path()).gap, 12);
        assert_eq!(t.library.load().unwrap()[0].id(), MEDIA_ID);
        assert_eq!(players::load(t.dir.path()).unwrap().profiles.len(), 1);

        let bare = write_backup(t.dir.path(), &json!([{ "id": B, "name": "Bare" }, null, "text", 3]));
        let res = import(&mut t.store, &mut t.library, &bare).unwrap();
        assert!(!res.layouts_changed);
        assert_eq!(t.store.load().unwrap().iter().map(|a| a.str("name").unwrap()).collect::<Vec<_>>(), ["Bare"]);
        assert_eq!(layouts::load(t.dir.path()).gap, 12);
        assert_eq!(t.library.load().unwrap().len(), 1);
    }

    #[test]
    fn version_3_replaces_the_library_and_profiles() {
        let mut t = source();
        let file = write_backup(t.dir.path(), &json!({ "version": 3, "apps": [], "layouts": {}, "library": [], "profiles": { "profiles": [], "defaults": {} } }));
        import(&mut t.store, &mut t.library, &file).unwrap();
        assert!(t.library.load().unwrap().is_empty());
        assert!(players::load(t.dir.path()).unwrap().profiles.is_empty());
        assert_eq!(layouts::load(t.dir.path()).layouts.len(), 5, "only the built-ins left");
    }

    #[test]
    fn a_layouts_or_media_failure_is_reported_without_undoing_the_apps() {
        let mut t = data();
        fs::create_dir_all(t.dir.path().join("layouts.json")).unwrap();
        fs::create_dir_all(t.dir.path().join("library.json")).unwrap();
        let file = write_backup(t.dir.path(), &json!({ "version": 3, "apps": [{ "id": A, "name": "Kept" }], "layouts": { "gap": 2 }, "library": [] }));
        let res = import(&mut t.store, &mut t.library, &file).unwrap();
        assert!(res.layouts_error.is_some() && !res.layouts_changed);
        assert!(res.media_error.is_some());
        assert_eq!(t.store.load().unwrap()[0].str("name"), Some("Kept"));
    }

    #[test]
    fn import_works_while_apps_json_is_read_only() {
        let mut t = data();
        fs::write(t.store.apps_file(), "{ not json").unwrap();
        t.store.load().unwrap();
        assert!(t.store.is_read_only());
        let file = write_backup(t.dir.path(), &json!([{ "id": A, "name": "Rescued" }]));
        import(&mut t.store, &mut t.library, &file).unwrap();
        assert!(!t.store.is_read_only());
        assert_eq!(t.store.load().unwrap()[0].str("name"), Some("Rescued"));
    }

    #[test]
    fn a_file_that_is_not_a_backup_is_refused_and_changes_nothing() {
        let mut t = source();
        let before = fs::read(t.store.apps_file()).unwrap();
        for text in ["not json", "{\"apps\": {}}", "{\"version\": 3}", "null", "5"] {
            let file = t.dir.path().join("bad.json");
            fs::write(&file, text).unwrap();
            let err = import(&mut t.store, &mut t.library, &file).unwrap_err();
            assert_eq!(err.to_string(), INVALID_BACKUP, "{text}");
        }
        let err = import(&mut t.store, &mut t.library, &t.dir.path().join("missing.json")).unwrap_err();
        assert_eq!(err.to_string(), INVALID_BACKUP);
        assert_eq!(fs::read(t.store.apps_file()).unwrap(), before);
    }

    #[test]
    fn copy_dir_skips_links() {
        let from = temp_dir();
        fs::write(from.path().join("f.txt"), b"f").unwrap();
        let outside = temp_dir();
        fs::write(outside.path().join("secret.txt"), b"s").unwrap();
        // Directory symlinks need a privilege on Windows; without it there is nothing to check.
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(outside.path(), from.path().join("link")).is_ok();
        #[cfg(not(windows))]
        let linked = std::os::unix::fs::symlink(outside.path(), from.path().join("link")).is_ok();
        let to = temp_dir();
        copy_dir(from.path(), &to.path().join("c")).unwrap();
        assert_eq!(crate::test_util::list_dir(&to.path().join("c")), ["f.txt"]);
        if !linked {
            eprintln!("symlinks unavailable; only the plain copy was checked");
        }
    }
}
