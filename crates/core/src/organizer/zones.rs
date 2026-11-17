//! Zone math for the window organizer. A layout divides a display's work area
//! into a cols x rows grid, and each zone is a block of whole cells - the same
//! integer-cell idea as `layout.rs` - so a layout means the same thing on
//! any resolution. Rects here are in DIP; the caller converts them to
//! physical pixels just before moving a window.
use super::{js_round, js_trim, whole_number, Rect};
use serde_json::{Map, Value};

pub const MAX_CELLS: i32 = 12;

/// A block of whole cells in its layout's grid. `extra` holds the keys this
/// version doesn't know, so they go back to the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub id: String,
    pub name: String,
    pub cols: i32,
    pub rows: i32,
    pub zones: Vec<Zone>,
    pub extra: Map<String, Value>,
}

fn cell(v: Option<&Value>) -> Option<i32> {
    whole_number(v).filter(|n| n.abs() <= f64::from(i32::MAX)).map(|n| n as i32)
}

fn unknown_keys(obj: &Map<String, Value>, known: &[&str]) -> Map<String, Value> {
    obj.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

impl Zone {
    pub fn new(id: &str, name: &str, x: i32, y: i32, w: i32, h: i32) -> Self {
        Zone { id: id.into(), name: name.into(), x, y, w, h, extra: Map::new() }
    }

    /// A zone of a layout that passed `validate_layout`. A name that is
    /// missing or not text reads as "".
    pub fn from_value(v: &Value) -> Option<Zone> {
        let obj = v.as_object()?;
        Some(Zone {
            id: obj.get("id")?.as_str()?.to_owned(),
            name: obj.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
            x: cell(obj.get("x"))?,
            y: cell(obj.get("y"))?,
            w: cell(obj.get("w"))?,
            h: cell(obj.get("h"))?,
            extra: unknown_keys(obj, &["id", "name", "x", "y", "w", "h"]),
        })
    }

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), self.id.as_str().into());
        m.insert("name".into(), self.name.as_str().into());
        for (key, n) in [("x", self.x), ("y", self.y), ("w", self.w), ("h", self.h)] {
            m.insert(key.into(), n.into());
        }
        m.extend(self.extra.clone());
        Value::Object(m)
    }
}

impl Layout {
    /// A layout that passed `validate_layout`; None for anything else.
    pub fn from_value(v: &Value) -> Option<Layout> {
        let obj = v.as_object()?;
        Some(Layout {
            id: obj.get("id")?.as_str()?.to_owned(),
            name: obj.get("name")?.as_str()?.to_owned(),
            cols: cell(obj.get("cols"))?,
            rows: cell(obj.get("rows"))?,
            zones: obj.get("zones")?.as_array()?.iter().map(Zone::from_value).collect::<Option<_>>()?,
            extra: unknown_keys(obj, &["id", "name", "cols", "rows", "zones"]),
        })
    }

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), self.id.as_str().into());
        m.insert("name".into(), self.name.as_str().into());
        m.insert("cols".into(), self.cols.into());
        m.insert("rows".into(), self.rows.into());
        m.insert("zones".into(), self.zones.iter().map(Zone::to_value).collect());
        m.extend(self.extra.clone());
        Value::Object(m)
    }
}

// same as layout::rects_overlap; merge after Phase 1
fn rects_overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    let ([ax, ay, aw, ah], [bx, by, bw, bh]) = (a, b);
    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
}

fn cell_count(v: Option<&Value>) -> Option<f64> {
    whole_number(v).filter(|n| *n >= 1.0 && *n <= f64::from(MAX_CELLS))
}

/// Returns None for a usable layout, or a short description of the first
/// problem found (used to skip bad entries in a hand-edited layouts.json).
/// Takes JSON because that is where wrong types come from.
pub fn validate_layout(layout: &Value) -> Option<String> {
    // An array is an object to JS, so it gets as far as the id check.
    if !layout.is_object() && !layout.is_array() {
        return Some("not an object".into());
    }
    if layout.get("id").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Some("missing id".into());
    }
    if layout.get("name").and_then(Value::as_str).is_none_or(|n| js_trim(n).is_empty()) {
        return Some("missing name".into());
    }
    let (Some(cols), Some(rows)) = (cell_count(layout.get("cols")), cell_count(layout.get("rows"))) else {
        return Some(format!("cols and rows must be whole numbers from 1 to {MAX_CELLS}"));
    };
    let zones = match layout.get("zones").and_then(Value::as_array) {
        Some(zones) if !zones.is_empty() => zones,
        _ => return Some("no zones".into()),
    };

    let mut seen: Vec<(&str, [f64; 4])> = Vec::new();
    for z in zones {
        let Some(id) = z.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()) else {
            return Some("a zone is missing its id".into());
        };
        if seen.iter().any(|(other, _)| *other == id) {
            return Some(format!("zone id \"{id}\" is used twice"));
        }
        let block = ["x", "y", "w", "h"].map(|key| whole_number(z.get(key)));
        let [Some(x), Some(y), Some(w), Some(h)] = block else {
            return Some(format!("zone \"{id}\" needs whole-number x, y, w and h"));
        };
        if x < 0.0 || y < 0.0 || w < 1.0 || h < 1.0 {
            return Some(format!("zone \"{id}\" needs whole-number x, y, w and h"));
        }
        if x + w > cols || y + h > rows {
            return Some(format!("zone \"{id}\" runs outside the {cols}x{rows} grid"));
        }
        seen.push((id, [x, y, w, h]));
    }
    for (i, (a, a_block)) in seen.iter().enumerate() {
        for (b, b_block) in &seen[i + 1..] {
            if rects_overlap(*a_block, *b_block) {
                return Some(format!("zones \"{a}\" and \"{b}\" overlap"));
            }
        }
    }
    None
}

/// The rect a window should fill for `zone` on a display with `work_area`.
/// `gap` separates neighbouring zones and also insets the outer edges, so a
/// window never touches the screen edge or its neighbour. Grid lines are
/// rounded once and shared by the zones on either side, and the gap is split
/// ceil/floor across them, so neighbours end up exactly `gap` apart even when
/// the work area doesn't divide evenly.
pub fn zone_rect(layout: &Layout, zone: &Zone, work_area: Rect, gap: i32) -> Rect {
    let (cols, rows, gap) = (f64::from(layout.cols), f64::from(layout.rows), f64::from(gap));
    let col_line = |i: i32| work_area.x + js_round(f64::from(i) * work_area.width / cols);
    let row_line = |i: i32| work_area.y + js_round(f64::from(i) * work_area.height / rows);
    let before = (gap / 2.0).ceil();
    let after = (gap / 2.0).floor();

    let left = col_line(zone.x) + if zone.x == 0 { gap } else { before };
    let right = col_line(zone.x + zone.w) - if zone.x + zone.w == layout.cols { gap } else { after };
    let top = row_line(zone.y) + if zone.y == 0 { gap } else { before };
    let bottom = row_line(zone.y + zone.h) - if zone.y + zone.h == layout.rows { gap } else { after };

    Rect { x: left, y: top, width: (right - left).max(1.0), height: (bottom - top).max(1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organizer::layouts::built_in_layouts;
    use serde_json::json;

    fn halves() -> Layout {
        built_in_layouts().into_iter().find(|l| l.id == "halves").unwrap()
    }
    fn right(r: Rect) -> f64 {
        r.x + r.width
    }
    fn bottom(r: Rect) -> f64 {
        r.y + r.height
    }

    // Work areas worth covering: a 1440p monitor under a 32px top bar, a second
    // monitor left of the primary (negative x), and sizes that don't divide
    // evenly by 2, 3 or 5.
    const WORK_AREAS: [Rect; 4] = [
        Rect::new(0.0, 32.0, 2560.0, 1408.0),
        Rect::new(-1920.0, 0.0, 1920.0, 1040.0),
        Rect::new(0.0, 0.0, 1366.0, 728.0),
        Rect::new(7.0, 3.0, 1001.0, 777.0),
    ];

    #[test]
    fn every_built_in_layout_is_valid() {
        for layout in built_in_layouts() {
            assert_eq!(validate_layout(&layout.to_value()), None, "{}", layout.id);
        }
    }

    #[test]
    fn zone_rect_halves_with_no_gap_split_the_work_area_exactly() {
        let (halves, wa) = (halves(), WORK_AREAS[0]);
        assert_eq!(zone_rect(&halves, &halves.zones[0], wa, 0), Rect::new(0.0, 32.0, 1280.0, 1408.0));
        assert_eq!(zone_rect(&halves, &halves.zones[1], wa, 0), Rect::new(1280.0, 32.0, 1280.0, 1408.0));
    }

    #[test]
    fn zone_rect_the_gap_insets_the_screen_edges_and_separates_neighbours_by_exactly_the_gap() {
        let (halves, wa) = (halves(), WORK_AREAS[0]);
        let l = zone_rect(&halves, &halves.zones[0], wa, 8);
        let r = zone_rect(&halves, &halves.zones[1], wa, 8);
        assert_eq!(l, Rect::new(8.0, 40.0, 1268.0, 1392.0));
        assert_eq!(r, Rect::new(1284.0, 40.0, 1268.0, 1392.0));
        assert_eq!(r.x - right(l), 8.0);

        // An odd gap is split 3 + 2 across the shared line, still 5 apart in total.
        let l5 = zone_rect(&halves, &halves.zones[0], wa, 5);
        let r5 = zone_rect(&halves, &halves.zones[1], wa, 5);
        assert_eq!(r5.x - right(l5), 5.0);
    }

    #[test]
    fn zone_rect_with_no_gap_every_built_in_tiles_the_work_area_with_no_overlap_or_holes() {
        for layout in built_in_layouts() {
            for wa in WORK_AREAS {
                let rects: Vec<Rect> = layout.zones.iter().map(|z| zone_rect(&layout, z, wa, 0)).collect();
                let area: f64 = rects.iter().map(|r| r.width * r.height).sum();
                assert_eq!(area, wa.width * wa.height, "{} on {wa:?}", layout.id);
                for r in &rects {
                    assert!(
                        r.x >= wa.x && r.y >= wa.y && right(*r) <= right(wa) && bottom(*r) <= bottom(wa),
                        "inside the work area"
                    );
                }
                for i in 0..rects.len() {
                    for j in i + 1..rects.len() {
                        let (a, b) = (rects[i], rects[j]);
                        let apart = right(a) <= b.x || right(b) <= a.x || bottom(a) <= b.y || bottom(b) <= a.y;
                        assert!(apart, "{}: zones {i} and {j} overlap", layout.id);
                    }
                }
            }
        }
    }

    #[test]
    fn zone_rect_with_a_gap_zones_keep_that_distance_from_the_edges_and_from_each_other() {
        for gap in [4, 8, 13] {
            let g = f64::from(gap);
            for layout in built_in_layouts() {
                for wa in WORK_AREAS {
                    let rects: Vec<Rect> = layout.zones.iter().map(|z| zone_rect(&layout, z, wa, gap)).collect();
                    for r in &rects {
                        assert!(r.x - wa.x >= g && r.y - wa.y >= g, "{}: top/left edge", layout.id);
                        assert!(
                            right(wa) - right(*r) >= g && bottom(wa) - bottom(*r) >= g,
                            "{}: bottom/right edge",
                            layout.id
                        );
                    }
                    for i in 0..rects.len() {
                        for j in i + 1..rects.len() {
                            let (a, b) = (rects[i], rects[j]);
                            let dx = (b.x - right(a)).max(a.x - right(b));
                            let dy = (b.y - bottom(a)).max(a.y - bottom(b));
                            let apart = dx.max(dy);
                            assert!(apart >= g, "{} gap {gap}: zones {i} and {j} are {apart} apart", layout.id);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn validate_layout_rejects_layouts_that_would_place_windows_badly() {
        let with = |key: &str, value: Value| {
            let mut v = halves().to_value();
            v[key] = value;
            v
        };
        let with_zones = |zones: Value| with("zones", zones);
        let z = |id: &str, x: f64, y: f64, w: f64, h: f64| json!({ "id": id, "x": x, "y": y, "w": w, "h": h });
        let problem = |v: Value| validate_layout(&v).unwrap_or_default();

        assert!(problem(Value::Null).contains("not an object"));
        assert!(problem(with("id", json!(""))).contains("missing id"));
        assert!(problem(with("name", json!("  "))).contains("missing name"));
        assert!(problem(with("cols", json!(0))).contains("cols and rows"));
        assert!(problem(with("rows", json!(13))).contains("cols and rows"));
        assert!(problem(with("cols", json!(1.5))).contains("cols and rows"));
        assert!(problem(with_zones(json!([]))).contains("no zones"));
        assert!(problem(with_zones(json!([z("a", 0.0, 0.0, 1.0, 1.0), z("a", 1.0, 0.0, 1.0, 1.0)]))).contains("used twice"));
        assert!(problem(with_zones(json!([z("a", 0.0, 0.0, 0.5, 1.0)]))).contains("whole-number"));
        assert!(problem(with_zones(json!([z("a", -1.0, 0.0, 1.0, 1.0)]))).contains("whole-number"));
        assert!(problem(with_zones(json!([z("a", 1.0, 0.0, 2.0, 1.0)]))).contains("outside the 2x1 grid"));
        assert!(problem(with_zones(json!([z("a", 0.0, 0.0, 2.0, 1.0), z("b", 1.0, 0.0, 1.0, 1.0)])))
            .contains("\"a\" and \"b\" overlap"));
        assert_eq!(
            validate_layout(&with_zones(json!([z("only", 0.0, 0.0, 1.0, 1.0)]))),
            None,
            "a layout may leave cells empty"
        );
    }

    // Not in the JS tests: the port's own rounding and JSON conversion.

    #[test]
    fn grid_lines_are_rounded_as_offsets_from_the_work_area_edge() {
        // The middle line of 1001 px is at 500.5 from the left edge, which
        // rounds up to 501 wherever the display is.
        let halves = halves();
        let wa = Rect::new(-1001.0, 0.0, 1001.0, 500.0);
        let l = zone_rect(&halves, &halves.zones[0], wa, 0);
        let r = zone_rect(&halves, &halves.zones[1], wa, 0);
        assert_eq!((l.x, l.width), (-1001.0, 501.0)); // round(500.5) = 501
        assert_eq!((r.x, r.width), (-500.0, 500.0));
    }

    #[test]
    fn layouts_round_trip_through_json_with_their_unknown_keys() {
        let v = json!({
            "id": "u", "name": "Mine", "cols": 2.0, "rows": 1, "colour": "red",
            "zones": [{ "id": "a", "x": 0, "y": 0, "w": 2, "h": 1, "pinned": true }],
        });
        assert_eq!(validate_layout(&v), None);
        let layout = Layout::from_value(&v).unwrap();
        assert_eq!((layout.cols, layout.zones[0].w, layout.zones[0].name.as_str()), (2, 2, ""));
        assert_eq!(
            layout.to_value(),
            json!({
                "id": "u", "name": "Mine", "cols": 2, "rows": 1,
                "zones": [{ "id": "a", "name": "", "x": 0, "y": 0, "w": 2, "h": 1, "pinned": true }],
                "colour": "red",
            })
        );
    }

    #[test]
    fn validate_layout_reads_an_array_as_an_object_without_an_id() {
        assert_eq!(validate_layout(&json!([])).as_deref(), Some("missing id"));
        assert_eq!(validate_layout(&json!("halves")).as_deref(), Some("not an object"));
    }
}
