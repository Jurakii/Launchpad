//! The Phase 1 gate: files Launchpad 1.2.2 wrote load and save back
//! unchanged, its backups import the same, and a backup this build writes
//! imports in 1.2.2. The fixtures come from 1.2.2's own JS
//! (tests/fixtures/generate.js, which documents them); absolute paths in them
//! are `{{DATA}}`, swapped for a real folder here.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use launchpad_core::backup;
use launchpad_core::media::library::Library;
use launchpad_core::media::players;
use launchpad_core::organizer::layouts;
use launchpad_core::settings::{self, Settings};
use launchpad_core::store::Store;
use serde_json::Value;

const FILES: [&str; 5] = ["apps.json", "settings.json", "layouts.json", "library.json", "profiles.json"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn v122(rel: &str) -> PathBuf {
    fixtures().join("v1.2.2").join(rel)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("launchpad-compat-{}", launchpad_core::ids::new_uuid()));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// `dir` as it appears inside a JSON string.
fn escaped(dir: &Path) -> String {
    let s = serde_json::to_string(&dir.to_string_lossy()).unwrap();
    s[1..s.len() - 1].to_owned()
}

/// A file's text with `dir` written as {{DATA}}, as the fixtures have it.
fn tokenized(file: &Path, dir: &Path) -> String {
    fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display())).replace(&escaped(dir), "{{DATA}}")
}

fn list_files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let rel = format!("{prefix}{}", e.file_name().to_string_lossy());
        if e.file_type().unwrap().is_dir() {
            list_files(&e.path(), &format!("{rel}/"), out);
        } else {
            out.push(rel);
        }
    }
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    list_files(dir, "", &mut out);
    out.sort();
    out
}

/// Copies a fixture folder, putting `dir` where {{DATA}} is.
fn copy_in(from: &Path, to: &Path, dir: &Path) {
    for rel in files_under(from) {
        let (src, dest) = (from.join(&rel), to.join(&rel));
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        if rel.ends_with(".json") {
            fs::write(dest, fs::read_to_string(src).unwrap().replace("{{DATA}}", &escaped(dir))).unwrap();
        } else {
            fs::copy(src, dest).unwrap();
        }
    }
}

/// What 1.2.2 does on every start and edit: load each file and save it.
fn resave(dir: &Path) {
    let mut store = Store::new(dir);
    let apps = store.load().unwrap();
    store.save(&apps).unwrap();
    settings::save(dir, &Settings::new()).unwrap();
    layouts::save(dir, &layouts::load(dir)).unwrap();
    let mut library = Library::new(dir);
    let entries = library.load().unwrap();
    library.save(&entries).unwrap();
    let profiles = serde_json::to_value(players::load(dir).unwrap()).unwrap();
    players::save(dir, &profiles).unwrap();
}

fn assert_bytes(rust: &str, js: &str, what: &str) {
    if rust != js {
        let line = rust.lines().zip(js.lines()).position(|(a, b)| a != b).unwrap_or(0);
        panic!(
            "{what} differs from what 1.2.2 wrote, first at line {}:\n  rust: {}\n  js:   {}",
            line + 1,
            rust.lines().nth(line).unwrap_or("<end>"),
            js.lines().nth(line).unwrap_or("<end>")
        );
    }
}

#[test]
fn a_1_2_2_data_folder_loads_and_saves_back_byte_for_byte() {
    let dir = TempDir::new();
    copy_in(&v122("data"), dir.path(), dir.path());
    resave(dir.path());
    for f in FILES {
        // Byte for byte: these are files 1.2.2 itself wrote, and resaving
        // them in 1.2.2 leaves every byte alone (generate.js checks that).
        assert_bytes(&tokenized(&dir.path().join(f), dir.path()), &fs::read_to_string(v122("data").join(f)).unwrap(), f);
    }
    let mut expected = files_under(&v122("data"));
    expected.extend(["apps.json.bak".to_owned(), "library.json.bak".to_owned()]);
    expected.sort();
    assert_eq!(files_under(dir.path()), expected, "nothing else is written or removed");
}

// Numbers compared by value, so 2.0 == 2; everything else exactly, key order included.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_json(a, b)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((ka, va), (kb, vb))| ka == kb && same_json(va, vb))
        }
        _ => a == b,
    }
}

#[test]
fn hand_edited_files_come_back_as_1_2_2_resaves_them() {
    let dir = TempDir::new();
    copy_in(&v122("hand-edited/input"), dir.path(), dir.path());
    resave(dir.path());
    for f in FILES {
        let rust = tokenized(&dir.path().join(f), dir.path());
        let js = fs::read_to_string(v122("hand-edited/js-resaved").join(f)).unwrap();
        if f == "profiles.json" {
            assert_bytes(&rust, &js, "hand-edited profiles.json"); // nothing here 1.2.2 wouldn't write
            continue;
        }
        // Compared as values, numbers by value: these files have number forms
        // JSON.stringify never writes (2.0, 1e3, -0), which 1.2.2 writes as
        // 2, 1000, 0 and this build as 2.0, 1000.0, -0.0. Both read either.
        let rust: Value = serde_json::from_str(&rust).unwrap();
        let mut js: Value = serde_json::from_str(&js).unwrap();
        known_differences(f, &mut js);
        assert!(same_json(&rust, &js), "hand-edited {f}:\nrust: {rust:#}\njs (less the known differences): {js:#}");
    }
}

/// Where this build still writes something other than 1.2.2 does, applied
/// to 1.2.2's output. Each one is a bug to fix in its module (then remove it here).
fn known_differences(file: &str, js: &mut Value) {
    // store: an entry that is a JSON list is dropped; 1.2.2 keeps it (its
    // `typeof e === 'object'` lets lists through) and writes it back. Only a
    // hand edit can put one there, and the app can't show or use it.
    if file == "apps.json" {
        js.as_array_mut().unwrap().retain(|e| !e.is_array());
    }
}

// 1.2.2 reads and writes back a string with half a UTF-16 pair in it
// (`"a\ud800b"`); serde_json refuses the whole file, so this build treats an
// apps.json like that as unreadable: kept aside, and read-only without a
// backup. Nothing is lost (the file isn't overwritten) but nothing loads.
#[test]
fn known_difference_a_lone_surrogate_makes_apps_json_unreadable() {
    let dir = TempDir::new();
    fs::write(dir.path().join("apps.json"), r#"[{ "id": "x", "name": "a\ud800b" }]"#).unwrap();
    let mut store = Store::new(dir.path());
    assert!(store.load().unwrap().is_empty());
    assert!(store.is_read_only());
}

fn import_result(res: &backup::Imported) -> Value {
    serde_json::json!({ "storedMissing": res.stored_missing, "layoutsError": res.layouts_error, "mediaError": res.media_error })
}

/// Imports `backup_file` into a fresh folder with this build; returns it.
fn rust_import(backup_file: &Path) -> (TempDir, Value) {
    let dir = TempDir::new();
    let mut store = Store::new(dir.path());
    store.load().unwrap(); // as at startup, to match the JS run
    let mut library = Library::new(dir.path());
    let res = backup::import(&mut store, &mut library, backup_file).unwrap();
    (dir, import_result(&res))
}

#[test]
fn backups_from_1_2_2_import_as_they_do_there() {
    let backups = TempDir::new();
    copy_in(&v122("backups"), backups.path(), Path::new("C:\\old-install"));
    for name in ["v3", "v2", "bare"] {
        let (dir, result) = rust_import(&backups.path().join(format!("{name}.json")));
        let expected_dir = v122(&format!("imported/{name}"));
        let expected: Value = serde_json::from_str(&fs::read_to_string(expected_dir.join("result.json")).unwrap()).unwrap();
        assert_eq!(result["storedMissing"], expected["storedMissing"], "{name}");
        assert_eq!(result["layoutsError"], expected["layoutsError"], "{name}");
        assert_eq!(result["mediaError"], expected["mediaError"], "{name}");
        let files = files_under(dir.path()).into_iter().filter(|f| f != "result.json").collect::<Vec<_>>();
        assert_eq!(serde_json::to_value(&files).unwrap(), expected["files"], "{name}: the files written");
        for f in FILES.iter().filter(|f| expected_dir.join(f).exists()) {
            assert_bytes(&tokenized(&dir.path().join(f), dir.path()), &fs::read_to_string(expected_dir.join(f)).unwrap(), &format!("{name} import: {f}"));
        }
        // Icons and stored copies come back with the bytes they left with.
        for f in files.iter().filter(|f| !f.ends_with(".json")) {
            let source = v122("data").join(f);
            assert_eq!(fs::read(dir.path().join(f)).unwrap(), fs::read(&source).unwrap(), "{name}: {f}");
        }
    }
}

#[test]
fn an_export_from_this_build_matches_the_1_2_2_export() {
    let data = TempDir::new();
    copy_in(&v122("data"), data.path(), data.path());
    let out = TempDir::new();
    let file = out.path().join("v3.json");
    let res = backup::export(&mut Store::new(data.path()), &mut Library::new(data.path()), &file).unwrap();
    assert_eq!((res.stored_count, res.stored_missing.len()), (2, 0));
    assert_bytes(&tokenized(&file, data.path()), &fs::read_to_string(v122("backups/v3.json")).unwrap(), "v3.json");
    let copies: Vec<String> = files_under(&out.path().join("v3-files"));
    let expected: Vec<String> = files_under(&v122("backups/v3-files"));
    assert_eq!(copies, expected);
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
}

// "A backup written by either must import in the other": the other way
// round is the test above. Runs 1.2.2's own import, so it needs node and the
// repo's node_modules; it is skipped (with a note) without node.
#[test]
fn a_backup_from_this_build_imports_in_1_2_2() {
    if !node_available() {
        eprintln!("node is not on PATH: skipped the 1.2.2 import of a Rust backup");
        return;
    }
    let data = TempDir::new();
    copy_in(&v122("data"), data.path(), data.path());
    let out = TempDir::new();
    let file = out.path().join("rust-backup.json");
    backup::export(&mut Store::new(data.path()), &mut Library::new(data.path()), &file).unwrap();

    let js_dir = TempDir::new();
    let run = Command::new("node").arg(fixtures().join("js-import.js")).arg(&file).arg(js_dir.path()).output().unwrap();
    assert!(run.status.success(), "1.2.2 refused the backup: {}", String::from_utf8_lossy(&run.stderr));
    let js_result: Value = serde_json::from_slice(&run.stdout).unwrap();

    let (rust_dir, rust_result) = rust_import(&file);
    assert_eq!(js_result, rust_result);
    assert_eq!(files_under(js_dir.path()), files_under(rust_dir.path()));
    for f in files_under(js_dir.path()) {
        let (js, rust) = (js_dir.path().join(&f), rust_dir.path().join(&f));
        if f.ends_with(".json") {
            assert_bytes(&tokenized(&rust, rust_dir.path()), &tokenized(&js, js_dir.path()), &f);
        } else {
            assert_eq!(fs::read(rust).unwrap(), fs::read(js).unwrap(), "{f}");
        }
    }
    // And it is what 1.2.2 makes of its own backup of the same data.
    for f in ["apps.json", "layouts.json", "library.json", "profiles.json"] {
        assert_bytes(&tokenized(&js_dir.path().join(f), js_dir.path()), &fs::read_to_string(v122("imported/v3").join(f)).unwrap(), &format!("1.2.2 import of a Rust backup: {f}"));
    }
}

#[test]
fn same_json_compares_numbers_by_value() {
    let a: Value = serde_json::from_str(r#"{"a":[2.0,1e3],"b":null}"#).unwrap();
    assert!(same_json(&a, &serde_json::json!({ "a": [2, 1000], "b": null })));
    assert!(!same_json(&a, &serde_json::json!({ "b": null, "a": [2, 1000] })), "key order counts");
}
