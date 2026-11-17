//! Small app-level preferences that live outside the apps.json list (e.g.
//! window behavior) - needs to be readable synchronously by the main process
//! before any window exists.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::safe_file::{corrupt_copy_path, write_file_atomic};
use crate::store::{read_text, truthy};
use crate::{Error, Result};

/// settings.json as it is on disk: every key, known or not, in file order.
pub type Settings = Map<String, Value>;

pub fn settings_file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// Fails on filesystem errors other than "doesn't exist yet", so `save`
/// never merges into {} (and drops every other setting) just because the
/// file was briefly locked.
pub fn read(dir: &Path) -> Result<Settings> {
    let file = settings_file(dir);
    let raw = match read_text(&file) {
        Ok(raw) => raw,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::new()),
        Err(e) => return Err(e.into()),
    };
    if let Ok(Value::Object(parsed)) = serde_json::from_str(&raw) {
        return Ok(parsed);
    }

    // Unparseable: move it aside rather than letting the next save() quietly
    // replace it. Moved, not copied, so this happens once and the next save
    // starts a fresh file.
    let corrupt_copy = corrupt_copy_path(&file);
    fs::rename(&file, &corrupt_copy)
        .map_err(|e| Error::msg(format!("settings.json is unreadable and could not be moved aside: {e}")))?;
    eprintln!("settings.json is unreadable; moved it to {}", corrupt_copy.display());
    Ok(Settings::new())
}

pub fn load(dir: &Path) -> Settings {
    read(dir).unwrap_or_default() // locked right now - use defaults for this read only
}

pub fn save(dir: &Path, patch: &Settings) -> Result<Settings> {
    let mut merged = read(dir)?;
    for (key, value) in patch {
        merged.insert(key.clone(), value.clone());
    }
    write_file_atomic(&settings_file(dir), serde_json::to_string_pretty(&merged)?)?;
    Ok(merged)
}

pub const PANEL_MODES: [&str; 3] = ["always", "autoHide", "off"];
const DEFAULT_GRID_HOTKEY: &str = "CommandOrControl+Alt+G";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerOptions {
    pub enabled: bool,
    pub panel: String,
    pub snap_on_drag: bool,
    pub grid_hotkey: String,
}

/// The window organizer's settings, with defaults for anything missing or
/// unrecognized. organizerEnabled is the master switch; the panel, drag
/// snapping and the grid placer's hotkey are on by default once it's on.
pub fn organizer_options(s: &Settings) -> OrganizerOptions {
    let text = |key| s.get(key).and_then(Value::as_str);
    OrganizerOptions {
        enabled: truthy(s.get("organizerEnabled")),
        panel: text("organizerPanel").filter(|p| PANEL_MODES.contains(p)).unwrap_or("always").to_owned(),
        snap_on_drag: s.get("organizerSnapOnDrag") != Some(&Value::Bool(false)),
        grid_hotkey: text("organizerHotkey").filter(|h| !h.is_empty()).unwrap_or(DEFAULT_GRID_HOTKEY).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{list_dir, temp_dir};
    use serde_json::json;

    fn obj(v: Value) -> Settings {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn save_merges_into_the_existing_settings() {
        let dir = temp_dir();
        save(dir.path(), &obj(json!({ "hotkey": "Alt+Space" }))).unwrap();
        save(dir.path(), &obj(json!({ "startFullscreen": true }))).unwrap();
        assert_eq!(load(dir.path()), obj(json!({ "hotkey": "Alt+Space", "startFullscreen": true })));
    }

    #[test]
    fn a_corrupt_settings_json_is_moved_aside_not_silently_overwritten() {
        let dir = temp_dir();
        let file = settings_file(dir.path());
        fs::write(&file, r#"{"hotkey": "Alt+Sp"#).unwrap();

        assert_eq!(load(dir.path()), Settings::new());
        let copies: Vec<String> =
            list_dir(dir.path()).into_iter().filter(|f| f.starts_with("settings.corrupt-")).collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read_to_string(dir.path().join(&copies[0])).unwrap(), r#"{"hotkey": "Alt+Sp"#);

        save(dir.path(), &obj(json!({ "startFullscreen": true }))).unwrap();
        let on_disk: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(on_disk, json!({ "startFullscreen": true }));
    }

    #[test]
    fn organizer_options_off_until_enabled_then_panel_always_shown_drag_snapping_on_and_ctrl_alt_g_for_the_grid() {
        let dir = temp_dir();
        assert_eq!(
            organizer_options(&load(dir.path())),
            OrganizerOptions {
                enabled: false,
                panel: "always".into(),
                snap_on_drag: true,
                grid_hotkey: "CommandOrControl+Alt+G".into(),
            }
        );
        let patch = json!({
            "organizerEnabled": true,
            "organizerPanel": "autoHide",
            "organizerSnapOnDrag": false,
            "organizerHotkey": "Alt+F1",
        });
        save(dir.path(), &obj(patch)).unwrap();
        assert_eq!(
            organizer_options(&load(dir.path())),
            OrganizerOptions { enabled: true, panel: "autoHide".into(), snap_on_drag: false, grid_hotkey: "Alt+F1".into() }
        );
        assert_eq!(
            organizer_options(&obj(json!({ "organizerPanel": "sideways" }))).panel,
            "always",
            "unknown modes fall back"
        );
        assert_eq!(organizer_options(&obj(json!({ "organizerHotkey": "" }))).grid_hotkey, "CommandOrControl+Alt+G");
    }

    // Not in the JS tests: what JSON.stringify(merged, null, 2) wrote, key
    // order and unknown keys included.
    #[test]
    fn save_writes_the_file_the_js_build_writes() {
        let dir = temp_dir();
        let file = settings_file(dir.path());
        fs::write(&file, r#"{"zebra":[1,{"a":null}],"hotkey":"Alt+Space","empty":{}}"#).unwrap();
        save(dir.path(), &obj(json!({ "hotkey": "Alt+X", "added": 2.5 }))).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n  \"zebra\": [\n    1,\n    {\n      \"a\": null\n    }\n  ],\n  \"hotkey\": \"Alt+X\",\n  \"empty\": {},\n  \"added\": 2.5\n}"
        );
    }
}
