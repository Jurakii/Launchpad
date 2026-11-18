//! The grid's pure rules, from renderer.js: cell math, the island card's
//! layout, keyboard moves, the marquee, breadcrumb paths and menu targets.
use launchpad_core::items::{descendant_island_ids, AppForm};
use launchpad_core::store::Entry;

/// renderer.js CELL: every cell is this square, in logical px.
pub const CELL: f64 = 112.0;
/// CARD_MARGIN: shaved off an expanded island so cards don't touch.
pub const CARD_MARGIN: f64 = 14.0;
pub const TILE: f64 = 96.0;
/// .grid's padding: the canvas starts this far in.
pub const PAD: f64 = 20.0;
const HEADER_H: f64 = 43.0;
const BODY_PAD: f64 = 24.0;
const BODY_GAP: f64 = 8.0;
const NAME_LABEL_H: f64 = 14.0;

/// The cell a drop lands on, from canvas coordinates. 1.2 measured from the
/// grid element (the canvas plus its 20 px padding), kept for the same feel.
pub fn drop_cell(cx: f64, cy: f64) -> (f64, f64) {
    let c = |v: f64| (((v + PAD) / CELL - 0.5) + 0.5).floor().max(0.0);
    (c(cx), c(cy))
}

pub fn cell_num(e: &Entry, key: &str) -> f64 {
    e.num(key).unwrap_or(0.0)
}

/// What fits in an expanded island's card (buildIslandCard).
#[derive(Debug, PartialEq)]
pub struct CardLayout {
    pub icon: f64,
    pub cols: usize,
    pub capacity: usize,
    /// Column pitch and row pitch, for placing the icons.
    pub col_w: f64,
    pub row_h: f64,
}

pub fn card_layout(w: f64, h: f64, show_names: bool) -> CardLayout {
    let card_w = w * CELL - CARD_MARGIN;
    let card_h = h * CELL - CARD_MARGIN;
    let icon = (h * CELL / 3.0).round().clamp(28.0, 64.0);
    let body_w = card_w - BODY_PAD;
    let body_h = card_h - HEADER_H - BODY_PAD;
    let row_h = icon + BODY_GAP + if show_names { NAME_LABEL_H } else { 0.0 };
    let cols = (((body_w + BODY_GAP) / (icon + BODY_GAP)).floor() as usize).max(1);
    let rows = (((body_h + BODY_GAP) / row_h).floor() as usize).max(1);
    // grid-template-columns: repeat(auto-fill, minmax(icon, 1fr)): columns share the width.
    let col_w = (body_w - BODY_GAP * (cols as f64 - 1.0)) / cols as f64;
    CardLayout { icon, cols, capacity: cols * rows, col_w, row_h }
}

impl CardLayout {
    /// How many of `n` children show; the rest go into a "+N" tile.
    pub fn shown(&self, n: usize) -> usize {
        if n <= self.capacity {
            n
        } else {
            self.capacity.saturating_sub(1).max(1)
        }
    }

    /// The i-th icon's top-left inside the card body (centred in its column).
    pub fn pos(&self, i: usize) -> (f64, f64) {
        let (c, r) = (i % self.cols, i / self.cols);
        (c as f64 * (self.col_w + BODY_GAP) + (self.col_w - self.icon) / 2.0, r as f64 * self.row_h)
    }
}

/// moveSelection: arrow keys over the tiles in their drawn order.
/// `tops_lefts` are each element's (top, left); returns the new index.
pub fn move_focus(tops_lefts: &[(f64, f64)], cur: i32, dx: i32, dy: i32) -> i32 {
    if tops_lefts.is_empty() {
        return cur;
    }
    if cur < 0 || cur as usize >= tops_lefts.len() {
        return 0;
    }
    if dx != 0 {
        return (cur + dx).clamp(0, tops_lefts.len() as i32 - 1);
    }
    let (top, left) = tops_lefts[cur as usize];
    let mut best = -1;
    let (mut best_row, mut best_col) = (f64::INFINITY, f64::INFINITY);
    for (i, &(t, l)) in tops_lefts.iter().enumerate() {
        if (dy < 0 && t >= top) || (dy > 0 && t <= top) {
            continue;
        }
        let (rd, cd) = ((t - top).abs(), (l - left).abs());
        if rd < best_row || (rd == best_row && cd < best_col) {
            best = i as i32;
            best_row = rd;
            best_col = cd;
        }
    }
    if best == -1 {
        cur
    } else {
        best
    }
}

/// An element's box on the canvas: x, y, w, h.
pub type Rect = (f64, f64, f64, f64);

pub fn overlaps(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 && a.0 + a.2 > b.0 && a.1 < b.1 + b.3 && a.1 + a.3 > b.1
}

pub fn contains(r: Rect, x: f64, y: f64) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// getAncestors: the island chain from Home down to `id`, as (id, name).
pub fn ancestors(apps: &[Entry], id: Option<&str>) -> Vec<(String, String)> {
    let mut chain = Vec::new();
    let mut cur = id.map(str::to_owned);
    while let Some(c) = cur.filter(|c| !c.is_empty()) {
        let Some(f) = apps.iter().find(|a| a.str("id") == Some(&c) && a.str("kind") == Some("island")) else { break };
        if chain.iter().any(|(i, _): &(String, String)| i == &c) {
            break; // a hand-edited cycle
        }
        chain.insert(0, (c, f.str("name").unwrap_or("").to_owned()));
        cur = f.str("parentId").map(str::to_owned);
    }
    chain
}

/// getPathLabel.
pub fn path_label(apps: &[Entry], id: &str) -> String {
    let chain = ancestors(apps, Some(id));
    if chain.is_empty() {
        "Home".into()
    } else {
        chain.into_iter().map(|(_, n)| n).collect::<Vec<_>>().join(" / ")
    }
}

/// `localeCompare` on names, near enough: case-insensitive, then exact.
pub fn name_order(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    let (x, y) = (a.str("name").unwrap_or(""), b.str("name").unwrap_or(""));
    x.to_lowercase().cmp(&y.to_lowercase()).then(x.cmp(y))
}

/// moveTargets: (label, parent id or "" for Home, is current) for an item's
/// Move to list, leaving out the item and anything inside it.
pub fn move_targets(apps: &[Entry], item: &Entry) -> Vec<(String, String, bool)> {
    let id = item.str("id").unwrap_or("");
    let excluded = if item.str("kind") == Some("island") { descendant_island_ids(apps, id) } else { Default::default() };
    let parent = item.str("parentId").unwrap_or("");
    let mut islands: Vec<&Entry> = apps
        .iter()
        .filter(|a| a.str("kind") == Some("island"))
        .filter(|a| a.str("id").is_some_and(|i| i != id && !excluded.contains(i)))
        .collect();
    islands.sort_by(|a, b| name_order(a, b));
    let mut out = vec![("Home".to_owned(), String::new(), parent.is_empty())];
    for f in islands {
        let fid = f.str("id").unwrap_or("").to_owned();
        out.push((path_label(apps, &fid), fid.clone(), parent == fid));
    }
    out
}

/// quickAddFromPath's form for a dropped program or "Add as link".
pub fn link_form(path: &str, parent: &str) -> AppForm {
    let slash = path.rfind(['\\', '/']).map_or(0, |i| i + 1);
    let base = &path[slash..];
    let ext = base.rfind('.').map(|i| base[i + 1..].to_lowercase()).unwrap_or_default();
    let (type_, interpreter) = match ext.as_str() {
        "py" => ("script", "python"),
        "js" | "mjs" => ("script", "node"),
        _ => ("path", ""),
    };
    let name = if ext.is_empty() { base } else { &base[..base.len() - ext.len() - 1] };
    AppForm {
        name: name.to_owned(),
        type_: type_.into(),
        target: path.to_owned(),
        interpreter: interpreter.into(),
        parent_id: parent.to_owned(),
        ..Default::default()
    }
}

/// `toLocaleString()` for en-US: "11/17/2026, 3:04:05 PM", from a local-time
/// count of ms since 1970.
// shortcut: always the US form; Electron followed the system locale. Use GetDateFormatEx if someone asks.
pub fn us_date_time(local_ms: i64) -> String {
    let secs = local_ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, min, s) = (sod / 3600, sod / 60 % 60, sod % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{m}/{d}/{y}, {h12}:{min:02}:{s:02} {}", if h < 12 { "AM" } else { "PM" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn e(v: serde_json::Value) -> Entry {
        Entry::from_value(v).unwrap()
    }

    #[test]
    fn drop_cells() {
        assert_eq!(drop_cell(0.0, 0.0), (0.0, 0.0));
        assert_eq!(drop_cell(91.0, 92.0), (0.0, 1.0)); // 1.2's 20 px bias
        assert_eq!(drop_cell(-50.0, 300.0), (0.0, 2.0));
    }

    #[test]
    fn card_fits() {
        let l = card_layout(3.0, 2.0, false);
        assert_eq!((l.icon, l.cols, l.capacity), (64.0, 4, 8));
        assert_eq!(l.shown(8), 8);
        assert_eq!(l.shown(9), 7);
        let small = card_layout(2.0, 2.0, true);
        assert_eq!(small.shown(1), 1);
        assert_eq!(small.pos(0).1, 0.0);
    }

    #[test]
    fn keyboard() {
        // Two rows: (0,0) (0,112) (0,224) / (112,0) (112,112)
        let t = [(0.0, 0.0), (0.0, 112.0), (0.0, 224.0), (112.0, 0.0), (112.0, 112.0)];
        assert_eq!(move_focus(&t, -1, 1, 0), 0);
        assert_eq!(move_focus(&t, 0, 1, 0), 1);
        assert_eq!(move_focus(&t, 4, 1, 0), 4);
        assert_eq!(move_focus(&t, 2, 0, 1), 4);
        assert_eq!(move_focus(&t, 4, 0, -1), 1);
        assert_eq!(move_focus(&t, 0, 0, -1), 0);
    }

    #[test]
    fn paths_and_targets() {
        let apps = vec![
            e(json!({"id": "a", "kind": "island", "name": "Work", "parentId": null})),
            e(json!({"id": "b", "kind": "island", "name": "Docs", "parentId": "a"})),
            e(json!({"id": "x", "kind": "app", "name": "X", "parentId": "b"})),
        ];
        assert_eq!(path_label(&apps, "b"), "Work / Docs");
        assert_eq!(ancestors(&apps, Some("b")).len(), 2);
        let t = move_targets(&apps, &apps[2]);
        assert_eq!(t.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Home", "Work / Docs", "Work"]);
        assert!(t[1].2 && !t[0].2);
        // An island can't go into itself or what it holds.
        assert_eq!(move_targets(&apps, &apps[0]).len(), 1);
    }

    #[test]
    fn links() {
        let f = link_form(r"C:\x\tool.py", "isl");
        assert_eq!((f.name.as_str(), f.type_.as_str(), f.interpreter.as_str(), f.parent_id.as_str()), ("tool", "script", "python", "isl"));
        assert_eq!(link_form(r"C:\Apps\Notepad++.exe", "").name, "Notepad++");
        assert_eq!(link_form(r"C:\noext", "").name, "noext");
    }

    #[test]
    fn dates() {
        assert_eq!(us_date_time(0), "1/1/1970, 12:00:00 AM");
        assert_eq!(us_date_time(1_795_446_245_000), "11/23/2026, 3:04:05 PM");
    }
}
