//! "Reset Launchpad" (Settings → Danger zone): back to a first run, except
//! Media, which is kept.
//!
//! Order matters: the pre-reset copy is written (and fsynced) before anything
//! is deleted, and a failure writing it stops the reset. A failure part-way
//! through leaves that copy in place, so the reset can always be undone by
//! importing it from Settings → Backup.
//!
//! This only changes files. The caller (1.2.2's `settings:reset` in main.js)
//! must also:
//! - before: turn the window organizer off (its panels give their AppBar
//!   space back) and close the Media window;
//! - on an error: report the message and stop; nothing is relaunched;
//! - on success: clear the UI's own prefs (localStorage in 1.2.2, `ui.json`
//!   from 2.0), flush them to disk, then about 600 ms later quit for good
//!   and relaunch, so no cache, helper or docked panel is left half-reset.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::backup::{base64_encode, remove_path, spread};
use crate::js::{read_text, truthy};
use crate::safe_file::write_file_atomic;
use crate::Result;

/// settings.json keys that belong to Media, kept like its own files.
pub const MEDIA_SETTINGS_KEYS: [&str; 2] = ["controller", "libraries"];

/// What goes, removed outright (folders recursively). With apps.json gone
/// the next start is a real first run and seeds the default apps itself.
/// settings.json is rewritten instead, keeping Media's keys.
pub const REMOVED: [&str; 6] = ["apps.json", "apps.json.bak", "layouts.json", "icons", "files", "thumbs"];

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_str(&read_text(file).ok()?).ok()
}

/// `pre-reset-2026-01-02T03-04-05-000Z.json` in the data folder.
pub fn pre_reset_path(dir: &Path, now_ms: i64) -> PathBuf {
    let stamp = crate::time::iso_string(now_ms).replace([':', '.'], "-");
    dir.join(format!("pre-reset-{stamp}.json"))
}

/// A backup-shaped copy of what the reset will replace. Icons are embedded
/// like an export's, so importing it brings them back; stored files' copies
/// aren't (they can be gigabytes), and the dialog says to export for those.
pub fn write_pre_reset(dir: &Path, now_ms: i64) -> Result<PathBuf> {
    let list: Vec<Value> = match read_json(&dir.join("apps.json")) {
        Some(Value::Array(apps)) => apps
            .iter()
            .map(|a| {
                let mut entry = spread(a);
                let icon = a.get("iconPath").filter(|_| a.get("type").and_then(Value::as_str) != Some("stored"));
                if let Some(Ok(data)) = icon.and_then(Value::as_str).filter(|p| !p.is_empty()).map(fs::read) {
                    entry.insert("iconData".into(), base64_encode(&data).into());
                }
                entry.shift_remove("iconPath");
                Value::Object(entry)
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut bundle = Map::new();
    bundle.insert("version".into(), crate::backup::VERSION.into());
    bundle.insert("preReset".into(), true.into());
    bundle.insert("apps".into(), list.into());
    for (key, name) in [("layouts", "layouts.json"), ("settings", "settings.json")] {
        // `readJson(...) || undefined`: a missing, unreadable or falsy file is left out.
        if let Some(v) = read_json(&dir.join(name)).filter(|v| truthy(Some(v))) {
            bundle.insert(key.into(), v);
        }
    }
    let file = pre_reset_path(dir, now_ms);
    write_file_atomic(&file, serde_json::to_string_pretty(&Value::Object(bundle))?)?;
    Ok(file)
}

/// Runs the reset and returns the pre-reset copy's path.
pub fn reset(dir: &Path, now_ms: i64) -> Result<PathBuf> {
    let pre_reset = write_pre_reset(dir, now_ms)?;

    let mut kept = Map::new();
    if let Some(Value::Object(current)) = read_json(&dir.join("settings.json")) {
        for key in MEDIA_SETTINGS_KEYS {
            if let Some(v) = current.get(key) {
                kept.insert(key.into(), v.clone());
            }
        }
    }
    write_file_atomic(&dir.join("settings.json"), serde_json::to_string_pretty(&Value::Object(kept))?)?;

    for name in REMOVED {
        remove_path(&dir.join(name))?;
    }
    Ok(pre_reset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{list_dir, temp_dir, TempDir};
    use serde_json::json;

    fn setup() -> TempDir {
        let dir = temp_dir();
        let d = dir.path();
        let w = |name: &str, data: Value| fs::write(d.join(name), data.to_string()).unwrap();
        fs::create_dir_all(d.join("icons")).unwrap();
        fs::write(d.join("icons").join("a.png"), "png-bytes").unwrap();
        fs::create_dir_all(d.join("files").join("thumbs")).unwrap();
        fs::create_dir_all(d.join("media").join("x")).unwrap();
        w("apps.json", json!([{ "id": "a", "kind": "app", "name": "A", "iconPath": d.join("icons").join("a.png") }]));
        w("apps.json.bak", json!([]));
        w("layouts.json", json!({ "version": 1, "gap": 4, "layouts": [] }));
        w("settings.json", json!({ "hotkey": "Alt+Space", "controller": { "deadZone": 0.5 }, "libraries": [{ "root": "D:\\Games" }] }));
        w("library.json", json!([{ "id": "m" }]));
        w("profiles.json", json!({ "profiles": [] }));
        dir
    }

    // new Date('2026-01-02T03:04:05Z').getTime()
    const NOW: i64 = 1_767_323_045_000;

    #[test]
    fn reset_keeps_a_pre_reset_copy_clears_launchpad_back_to_a_first_run_and_leaves_media_alone() {
        let dir = setup();
        let pre_reset = reset(dir.path(), NOW).unwrap();
        let files = list_dir(dir.path());
        for gone in ["apps.json", "apps.json.bak", "layouts.json", "icons", "files"] {
            assert!(!files.iter().any(|f| f == gone), "{gone}");
        }
        for kept in ["library.json", "profiles.json", "media"] {
            assert!(files.iter().any(|f| f == kept), "{kept}");
        }
        let settings: Value = serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings, json!({ "controller": { "deadZone": 0.5 }, "libraries": [{ "root": "D:\\Games" }] }));
        assert_eq!(pre_reset.file_name().unwrap(), "pre-reset-2026-01-02T03-04-05-000Z.json");
        let pre: Value = serde_json::from_str(&fs::read_to_string(&pre_reset).unwrap()).unwrap();
        let keys: Vec<&str> = pre.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["version", "preReset", "apps", "layouts", "settings"]);
        assert_eq!(pre["apps"][0]["name"], "A");
        assert!(pre["apps"][0].get("iconPath").is_none());
        assert_eq!(crate::backup::base64_decode(pre["apps"][0]["iconData"].as_str().unwrap()), b"png-bytes");
        assert_eq!(pre["layouts"]["gap"], 4);
        assert_eq!(pre["settings"]["hotkey"], "Alt+Space");
    }

    #[test]
    fn the_pre_reset_copy_of_an_empty_or_broken_folder_is_still_written() {
        let dir = temp_dir();
        fs::write(dir.path().join("apps.json"), "{ broken").unwrap();
        fs::write(dir.path().join("settings.json"), "[1]").unwrap();
        let pre_reset = reset(dir.path(), NOW).unwrap();
        let pre: Value = serde_json::from_str(&fs::read_to_string(pre_reset).unwrap()).unwrap();
        assert_eq!(pre, json!({ "version": 3, "preReset": true, "apps": [], "settings": [1] }));
        assert_eq!(fs::read_to_string(dir.path().join("settings.json")).unwrap(), "{}");
        assert_eq!(list_dir(dir.path()).len(), 2);
    }

    // A file another process holds without delete sharing can't be removed,
    // which is the failure part-way the JS test fakes by stubbing fs.rmSync.
    #[cfg(windows)]
    #[test]
    fn a_failure_part_way_leaves_the_pre_reset_copy_in_place() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        let dir = setup();
        let lock = fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(dir.path().join("layouts.json")).unwrap();
        assert!(reset(dir.path(), NOW).is_err());
        drop(lock);
        let files = list_dir(dir.path());
        let pre = files.iter().find(|f| f.starts_with("pre-reset-")).expect("pre-reset copy kept");
        let pre: Value = serde_json::from_str(&fs::read_to_string(dir.path().join(pre)).unwrap()).unwrap();
        assert_eq!(pre["apps"][0]["name"], "A", "written before apps.json was deleted");
        assert!(!files.iter().any(|f| f == "apps.json"));
        assert!(files.iter().any(|f| f == "layouts.json"));
    }
}
