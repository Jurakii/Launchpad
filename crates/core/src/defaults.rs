//! The stock apps a fresh install starts with (File Explorer, then Recycle
//! Bin, the order Explorer's own sidebar uses), and the repair that puts them
//! back the way this version ships them. Kept free of UI and icon work so it
//! can be tested; the caller fetches the icons.
use serde_json::json;

use crate::layout;
use crate::store::Entry;

// Recycle Bin isn't a real file (it's a virtual shell folder), so it's a
// custom command opening the special shell namespace path, and the renderer
// draws its icon (`special`) rather than one extracted from a target.
pub const EXPLORER_PATH: &str = "C:\\Windows\\explorer.exe";
pub const RECYCLE_BIN_CLSID: &str = "::{645FF040-5081-101B-9F08-00AA002F954E}";

pub struct StockDefault {
    /// How a default is found again in apps.json: not by name, which the
    /// user can change.
    pub key: &'static str,
    pub name: &'static str,
    /// The entry's `type`.
    pub type_: &'static str,
    pub target: &'static str,
    pub command: &'static str,
    pub special: Option<&'static str>,
}

pub const DEFAULTS: [StockDefault; 2] = [
    StockDefault {
        key: "explorer",
        name: "File Explorer",
        type_: "path",
        target: EXPLORER_PATH,
        command: "",
        special: None,
    },
    StockDefault {
        key: "recycleBin",
        name: "Recycle Bin",
        type_: "command",
        target: "",
        // explorer.exe + RECYCLE_BIN_CLSID
        command: "explorer.exe ::{645FF040-5081-101B-9F08-00AA002F954E}",
        // Windows won't hand over its real recycle-bin icon through any path
        // form app.getFileIcon understands (tried the CLSID, shell: URI, the
        // registry-documented imageres.dll,-55 resource, and the raw $Recycle.Bin
        // folder - all just return a generic file/folder icon). The renderer
        // draws a proper trash-can glyph for any item flagged like this instead.
        special: Some("recycleBin"),
    },
];

/// Entries carry `stockKey` from this version on. Older ones are recognised
/// by what they launch (Explorer's path, the Recycle Bin's flag), and failing
/// that by the stock name, so a broken target can still be repaired.
pub fn matches_default(entry: &Entry, def: &StockDefault) -> bool {
    if entry.str("kind") != Some("app") {
        return false;
    }
    if entry.truthy("stockKey") {
        return entry.str("stockKey") == Some(def.key);
    }
    if let Some(special) = def.special {
        return entry.str("special") == Some(special);
    }
    if entry.truthy("special") {
        return false;
    }
    let lower = |key| entry.str(key).map(str::to_lowercase).unwrap_or_default();
    lower("target") == def.target.to_lowercase() || lower("name") == def.name.to_lowercase()
}

/// Fresh entries for a first run, laid out explicitly (Explorer at 0,0, the
/// Recycle Bin beside it) rather than left to array or createdAt order.
/// `now` is `time::now_ms()` outside tests.
pub fn build_entries(mut new_id: impl FnMut() -> String, now: i64) -> Vec<Entry> {
    (0i64..)
        .zip(&DEFAULTS)
        .map(|(i, def)| {
            let mut entry = Entry::from_value(json!({
                "id": new_id(),
                "kind": "app",
                "name": def.name,
                "type": def.type_,
                "target": def.target,
                "args": "",
                "cwd": "",
                "interpreter": "",
                "command": def.command,
                "parentId": null,
                "pinned": false,
                "createdAt": now + i, // Explorer is the older one
                "lastLaunched": null,
                "launchCount": 0,
                "color": null,
                "iconPath": null,
                "iconIsCustom": false,
                "gridX": i,
                "gridY": 0,
                "stockKey": def.key,
            }))
            .expect("an object");
            if let Some(special) = def.special {
                entry.set("special", special);
            }
            entry
        })
        .collect()
}

/// The entries `repair` touched, as indexes into `apps`, for the caller to
/// refresh icons on.
#[derive(Debug, Default, PartialEq)]
pub struct Repair {
    pub repaired: Vec<usize>,
    pub added: Vec<usize>,
}

/// Repairs the stock apps in `apps` (mutated in place): each one found by
/// its key gets this version's type, target and command back (its name,
/// island, position, pin and colour are the user's and stay), and any that's
/// missing is added at Home on the nearest free cell to where a first run
/// puts it.
pub fn repair(apps: &mut Vec<Entry>, new_id: impl FnMut() -> String, now: i64) -> Repair {
    let fresh = build_entries(new_id, now);
    let mut result = Repair::default();
    for (def, mut entry) in DEFAULTS.iter().zip(fresh) {
        let existing: Vec<usize> = (0..apps.len()).filter(|&i| matches_default(&apps[i], def)).collect();
        if existing.is_empty() {
            let target = entry.num("gridX").zip(entry.num("gridY"));
            layout::place_free(apps, &mut entry, target);
            apps.push(entry);
            result.added.push(apps.len() - 1);
            continue;
        }
        for i in existing {
            let entry = &mut apps[i];
            entry.set("stockKey", def.key);
            entry.set("type", def.type_);
            entry.set("target", def.target);
            entry.set("command", def.command);
            match def.special {
                Some(special) => entry.set("special", special),
                None => entry.remove("special"),
            }
            result.repaired.push(i);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("00000000-0000-4000-8000-{n:012}")
        }
    }

    fn grid(e: &Entry) -> (f64, f64) {
        (e.num("gridX").unwrap(), e.num("gridY").unwrap())
    }

    #[test]
    fn first_run_file_explorer_is_at_the_first_cell_and_the_recycle_bin_beside_it_explorer_the_older() {
        let entries = build_entries(counter(), 1000);
        let [explorer, bin] = &entries[..] else { panic!("two entries") };
        assert_eq!(explorer.str("name"), Some("File Explorer"));
        assert_eq!(grid(explorer), (0.0, 0.0));
        assert_eq!(bin.str("name"), Some("Recycle Bin"));
        assert_eq!(grid(bin), (1.0, 0.0));
        assert!(explorer.num("createdAt").unwrap() < bin.num("createdAt").unwrap());
        assert_eq!(bin.str("special"), Some("recycleBin"));
        assert_eq!(explorer.get("special"), None);
    }

    #[test]
    fn repair_fixes_a_broken_explorer_entry_found_by_its_key_keeping_name_island_position_pin_and_colour() {
        let mut apps = build_entries(counter(), crate::time::now_ms());
        for (key, value) in [
            ("name", json!("Files")),
            ("target", json!("C:\\nowhere.exe")),
            ("type", json!("url")),
            ("parentId", json!("island-1")),
            ("gridX", json!(4)),
            ("gridY", json!(2)),
            ("pinned", json!(true)),
            ("color", json!("#f00")),
        ] {
            apps[0].set(key, value);
        }
        let result = repair(&mut apps, counter(), crate::time::now_ms());
        assert_eq!(result.added.len(), 0);
        assert_eq!(result.repaired.len(), 2);
        let explorer = &apps[0];
        assert_eq!(explorer.str("target"), Some(EXPLORER_PATH));
        assert_eq!(explorer.str("type"), Some("path"));
        let kept: Vec<&Value> =
            ["name", "parentId", "gridX", "gridY", "pinned", "color"].iter().map(|k| explorer.get(k).unwrap()).collect();
        assert_eq!(kept, [&json!("Files"), &json!("island-1"), &json!(4), &json!(2), &json!(true), &json!("#f00")]);
    }

    #[test]
    fn repair_recognises_entries_from_before_stock_key_renamed_ones_included_and_adds_missing_defaults_on_free_cells() {
        let mut new_id = counter();
        let mut apps: Vec<Entry> = [
            json!({ "id": new_id(), "kind": "app", "name": "My Explorer", "type": "path", "target": "c:\\windows\\EXPLORER.EXE", "parentId": null, "gridX": 1, "gridY": 0 }),
            json!({ "id": new_id(), "kind": "app", "name": "Notes", "type": "path", "target": "C:\\notes.exe", "parentId": null, "gridX": 0, "gridY": 0 }),
        ]
        .into_iter()
        .map(|v| Entry::from_value(v).unwrap())
        .collect();
        let result = repair(&mut apps, new_id, crate::time::now_ms());
        let names = |idx: &[usize]| idx.iter().map(|&i| apps[i].str("name").unwrap().to_owned()).collect::<Vec<_>>();
        assert_eq!(names(&result.repaired), ["My Explorer"]);
        assert_eq!(apps[result.repaired[0]].str("stockKey"), Some("explorer"));
        assert_eq!(names(&result.added), ["Recycle Bin"]);
        let bin = result.added[0];
        assert!(!apps.iter().enumerate().any(|(i, a)| i != bin && grid(a) == grid(&apps[bin])));
        let notes = apps.iter().find(|a| a.str("name") == Some("Notes")).unwrap();
        assert_eq!(notes.str("target"), Some("C:\\notes.exe")); // user apps untouched
    }

    // Not in the JS tests: the JS builds the command from the CLSID, and a
    // first-run entry has to serialize with the JS key order.
    #[test]
    fn first_run_entries_have_the_js_shape() {
        assert_eq!(DEFAULTS[1].command, format!("explorer.exe {RECYCLE_BIN_CLSID}"));
        let bin = &build_entries(|| "x".to_owned(), 5)[1];
        assert_eq!(
            serde_json::to_string(bin).unwrap(),
            r#"{"id":"x","kind":"app","name":"Recycle Bin","type":"command","target":"","args":"","cwd":"","interpreter":"","command":"explorer.exe ::{645FF040-5081-101B-9F08-00AA002F954E}","parentId":null,"pinned":false,"createdAt":6,"lastLaunched":null,"launchCount":0,"color":null,"iconPath":null,"iconIsCustom":false,"gridX":1,"gridY":0,"stockKey":"recycleBin","special":"recycleBin"}"#
        );
    }
}
