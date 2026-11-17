//! Media's controller settings, kept in settings.json under `controller`:
//! the stick dead zone and a button map per pad (keyed by the Gamepad id
//! string, so a second, different pad keeps its own). A pad with no map uses
//! pad.rs's standard (Xbox) mapping. Ported from lib/media/controller.js.
use crate::js::{js_to_number, string_to_number};
use std::path::Path;

use serde_json::{json, Map, Value};

use super::pad::Mapping;
pub use super::pad::DEFAULT_DEAD_ZONE;
use crate::js::{js_num, js_round};
use crate::settings;
use crate::Result;

pub const ACTIONS: [&str; 13] = [
    "select", "back", "quick", "customize", "tabPrev", "tabNext", "pageUp", "pageDown", "menu", "up", "down", "left",
    "right",
];
const MAX_PADS: usize = 16;
const MAX_BUTTON: f64 = 31.0;

/// What `load` and `save` return: `{ deadZone, pads }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    pub dead_zone: f64,
    /// Pad id -> its map, in the order settings.json lists them.
    pub pads: Vec<(String, Mapping)>,
}

impl Controller {
    /// The map saved for one pad, if any.
    pub fn mapping_for(&self, id: &str) -> Option<&Mapping> {
        self.pads.iter().find(|(p, _)| p == id).map(|(_, m)| m)
    }

    /// The object written to settings.json's `controller`.
    pub fn to_json(&self) -> Value {
        let pads: Map<String, Value> = self
            .pads
            .iter()
            .map(|(id, m)| {
                let map: Map<String, Value> = m.iter().map(|(b, a)| (b.to_string(), Value::from(a.as_str()))).collect();
                (id.clone(), Value::Object(map))
            })
            .collect();
        json!({ "deadZone": js_num(self.dead_zone), "pads": pads })
    }
}

/// The dead zone, clamped to 10-90% in 5% steps.
fn clean_dead_zone(value: Option<&Value>) -> f64 {
    let n = js_to_number(value);
    if !n.is_finite() {
        return DEFAULT_DEAD_ZONE;
    }
    js_round(n.clamp(0.1, 0.9) * 20.0) / 20.0
}

// Object.entries order: integer-index keys ascending first, then the rest
// in insertion order.
fn js_entries(map: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let index = |k: &str| k.parse::<u32>().ok().filter(|n| *n != u32::MAX && n.to_string() == k);
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(k, _)| index(k).map_or((1, 0), |n| (0, n)));
    entries
}

/// `{ "<button index>": action }`, valid buttons and actions only, and each
/// action on one button at most (the first one listed wins).
fn clean_mapping(mapping: Option<&Value>) -> Mapping {
    let mut out = Mapping::new();
    let mut used: Vec<&str> = Vec::new();
    let Some(Value::Object(mapping)) = mapping else { return out };
    for (key, action) in js_entries(mapping) {
        let button = string_to_number(key);
        if button.fract() != 0.0 || !(0.0..=MAX_BUTTON).contains(&button) {
            continue; // also NaN and the infinities
        }
        let Some(action) = action.as_str().filter(|a| ACTIONS.contains(a)) else { continue };
        if used.contains(&action) {
            continue;
        }
        used.push(action);
        out.insert(button as u32, action.to_owned()); // "01" lands on button 1 as JS's out[1] does
    }
    out
}

pub fn sanitize(data: Option<&Value>) -> Controller {
    let src = data.and_then(Value::as_object);
    let mut pads = Vec::new();
    // `typeof === 'object'` lets an array through: its indices become ids.
    // shortcut: pad ids that look like array indices keep file order here;
    // JS lists them first. Gamepad ids are never bare numbers.
    let entries: Vec<(String, &Value)> = match src.and_then(|s| s.get("pads")) {
        Some(Value::Object(p)) => p.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Some(Value::Array(a)) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        _ => Vec::new(),
    };
    for (id, mapping) in entries.into_iter().take(MAX_PADS) {
        if id.is_empty() || crate::js::utf16_len(&id) > 200 {
            continue;
        }
        let clean = clean_mapping(Some(mapping));
        // A map without select and back would leave the pad unable to get out.
        let has = |a: &str| clean.values().any(|v| v == a);
        if has("select") && has("back") {
            pads.push((id, clean));
        }
    }
    Controller { dead_zone: clean_dead_zone(src.and_then(|s| s.get("deadZone"))), pads }
}

pub fn load(dir: &Path) -> Controller {
    sanitize(settings::load(dir).get("controller"))
}

pub fn save(dir: &Path, data: &Value) -> Result<Controller> {
    let clean = sanitize(Some(data));
    let mut patch = settings::Settings::new();
    patch.insert("controller".into(), clean.to_json());
    settings::save(dir, &patch)?;
    Ok(clean)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_dir;

    const PAD: &str = "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 02fd)";

    fn mapping(pairs: &[(u32, &str)]) -> Mapping {
        pairs.iter().map(|(b, a)| (*b, a.to_string())).collect()
    }

    #[test]
    fn defaults_when_nothing_is_saved() {
        let dir = temp_dir();
        assert_eq!(load(dir.path()), Controller { dead_zone: 0.35, pads: vec![] });
    }

    #[test]
    fn the_dead_zone_is_clamped_to_10_90_percent_in_5_percent_steps() {
        let dz = |v: Value| sanitize(Some(&json!({ "deadZone": v }))).dead_zone;
        assert_eq!(dz(json!(0.02)), 0.1);
        assert_eq!(dz(json!(2)), 0.9);
        assert_eq!(dz(json!(0.42)), 0.4);
        assert_eq!(dz(json!("x")), 0.35);
    }

    #[test]
    fn mappings_keep_valid_buttons_and_actions_one_button_per_action() {
        let data = json!({ "pads": { PAD: { "1": "select", "0": "back", "2": "select", "40": "menu", "-1": "menu", "3": "fly", "x": "up", "12": "up" } } });
        let c = sanitize(Some(&data));
        assert_eq!(c.mapping_for(PAD), Some(&mapping(&[(0, "back"), (1, "select"), (12, "up")])));
    }

    #[test]
    fn a_pad_map_without_select_or_back_is_dropped_so_the_pad_keeps_working() {
        let data = json!({ "pads": { PAD: { "0": "select", "2": "menu" }, "": { "0": "select", "1": "back" } } });
        assert_eq!(sanitize(Some(&data)).pads, vec![]);
    }

    #[test]
    fn save_round_trips_through_settings_json() {
        let dir = temp_dir();
        save(dir.path(), &json!({ "deadZone": 0.5, "pads": { PAD: { "1": "select", "0": "back" } } })).unwrap();
        let want = Controller { dead_zone: 0.5, pads: vec![(PAD.to_owned(), mapping(&[(0, "back"), (1, "select")]))] };
        assert_eq!(load(dir.path()), want);
    }

    // Not in the JS tests: Number() on keys and values the way JS reads
    // them, and the JSON settings.json gets (checked against node).
    #[test]
    fn js_number_rules_for_keys_and_the_dead_zone() {
        let pad = json!({ "x": "up", "1": "quick", "01": "back", " 2 ": "menu", "0x3": "select", "": "left", "1e1": "right", "Infinity": "down" });
        let c = sanitize(Some(&json!({ "pads": { "p": pad } })));
        assert_eq!(c.mapping_for("p"), Some(&mapping(&[(0, "left"), (1, "back"), (2, "menu"), (3, "select"), (10, "right")])));
        let c = sanitize(Some(&json!({ "pads": [{ "0": "select", "1": "back" }] })));
        assert_eq!(c.pads, vec![("0".to_owned(), mapping(&[(0, "select"), (1, "back")]))]);
        let dz = |v: Value| sanitize(Some(&json!({ "deadZone": v }))).dead_zone;
        assert_eq!(dz(json!(null)), 0.1);
        assert_eq!(dz(json!(true)), 0.9);
        assert_eq!(dz(json!(" 0.66 ")), 0.65);
        assert_eq!(dz(json!("inf")), 0.35);
        assert_eq!(dz(json!(0.375)), 0.4);
        let c = sanitize(Some(&json!({ "deadZone": 0.5, "pads": { PAD: { "1": "select", "0": "back" } } })));
        assert_eq!(c.to_json().to_string(), format!(r#"{{"deadZone":0.5,"pads":{{"{PAD}":{{"0":"back","1":"select"}}}}}}"#));
    }
}
