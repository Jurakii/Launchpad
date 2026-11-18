//! The taskbar's lists, as renderer.js builds them: the pinned row
//! (`renderPinned`), its drag-to-reorder (the pinned row's `drop`), the
//! search popout (`taskbarSearch` input) and Recent (`recentBtn` click).
use crate::js::js_trim;
use crate::store::Entry;

/// Search shows at most this many matches, Recent this many items.
pub const SEARCH_MAX: usize = 20;
pub const RECENT_MAX: usize = 8;

/// The pinned items in taskbar order (pinOrder, a missing one last). With
/// the buttons on the right the row is mirrored, so the order is reversed.
pub fn pinned(apps: &[Entry], mirrored: bool) -> Vec<&Entry> {
    let mut list: Vec<&Entry> = apps.iter().filter(|a| a.truthy("pinned")).collect();
    // Stable, as Array.prototype.sort is.
    list.sort_by(|a, b| order(a).total_cmp(&order(b)));
    if mirrored {
        list.reverse();
    }
    list
}

fn order(a: &Entry) -> f64 {
    a.num("pinOrder").unwrap_or(f64::INFINITY)
}

/// The order to save after dragging the tile at `from` of `displayed` (the
/// row as shown) into gap `gap` (0 = before the first tile, len = after the
/// last). Saved unmirrored, so a drag while mirrored never bakes the mirror in.
pub fn reorder(displayed: &[String], from: usize, gap: usize, mirrored: bool) -> Vec<String> {
    let mut order = displayed.to_vec();
    if from >= order.len() {
        return order;
    }
    let id = order.remove(from);
    // Removing the dragged tile shifted everything after it left.
    let at = if from < gap { gap - 1 } else { gap };
    order.insert(at.min(order.len()), id);
    if mirrored {
        order.reverse();
    }
    order
}

/// Every item (in any island) whose name contains the trimmed query,
/// ignoring case, in apps.json order. Nothing for a blank query.
pub fn search<'a>(apps: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let q = js_trim(query).to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    apps.iter().filter(|a| a.str("name").unwrap_or("").to_lowercase().contains(&q)).take(SEARCH_MAX).collect()
}

/// A query `search` finds nothing for (1.2 hid the results then).
pub fn is_blank(query: &str) -> bool {
    js_trim(query).is_empty()
}

/// The apps launched most recently, newest first.
pub fn recent(apps: &[Entry]) -> Vec<&Entry> {
    let launched = |a: &Entry| a.num("lastLaunched").filter(|&t| t != 0.0 && !t.is_nan());
    let mut list: Vec<&Entry> = apps.iter().filter(|a| a.str("kind") == Some("app") && launched(a).is_some()).collect();
    list.sort_by(|a, b| launched(b).unwrap_or(0.0).total_cmp(&launched(a).unwrap_or(0.0)));
    list.truncate(RECENT_MAX);
    list
}

/// Where an item lives, for the popout rows: "Home", or its islands from
/// the top down ("Work / Tools").
pub fn path_label(apps: &[Entry], island_id: Option<&str>) -> String {
    let mut chain = Vec::new();
    let mut cur = island_id.filter(|s| !s.is_empty());
    // Bounded: a hand-edited file could make islands each other's parents.
    while let Some(id) = cur.filter(|_| chain.len() <= apps.len()) {
        let Some(f) = apps.iter().find(|a| a.str("id") == Some(id) && a.str("kind") == Some("island")) else { break };
        chain.push(f.str("name").unwrap_or(""));
        cur = f.str("parentId").filter(|s| !s.is_empty());
    }
    if chain.is_empty() {
        return "Home".into();
    }
    chain.reverse();
    chain.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn list(vs: Vec<serde_json::Value>) -> Vec<Entry> {
        vs.into_iter().map(|v| Entry::from_value(v).unwrap()).collect()
    }

    fn ids(v: &[&Entry]) -> Vec<String> {
        v.iter().map(|e| e.str("id").unwrap().to_string()).collect()
    }

    #[test]
    fn pinned_sorts_by_pin_order_with_missing_last_and_mirrors() {
        let apps = list(vec![
            json!({ "id": "a", "pinned": true }),
            json!({ "id": "b", "pinned": true, "pinOrder": 2 }),
            json!({ "id": "c", "pinned": false, "pinOrder": 0 }),
            json!({ "id": "d", "pinned": true, "pinOrder": 1 }),
        ]);
        assert_eq!(ids(&pinned(&apps, false)), ["d", "b", "a"]);
        assert_eq!(ids(&pinned(&apps, true)), ["a", "b", "d"]);
    }

    #[test]
    fn reorder_moves_into_the_gap_and_saves_unmirrored() {
        let row: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        assert_eq!(reorder(&row, 0, 4, false), ["b", "c", "d", "a"]);
        assert_eq!(reorder(&row, 0, 2, false), ["b", "a", "c", "d"]);
        assert_eq!(reorder(&row, 3, 0, false), ["d", "a", "b", "c"]);
        assert_eq!(reorder(&row, 1, 1, false), row); // dropped where it was
        assert_eq!(reorder(&row, 1, 2, false), row);
        assert_eq!(reorder(&row, 3, 0, true), ["c", "b", "a", "d"]);
        assert_eq!(reorder(&row, 9, 0, false), row);
    }

    #[test]
    fn search_matches_names_anywhere_ignoring_case_and_caps_at_20() {
        let mut vs = vec![
            json!({ "id": "x", "name": "Notepad++", "parentId": "isl" }),
            json!({ "id": "isl", "kind": "island", "name": "Note stuff" }),
            json!({ "id": "y", "name": 5 }),
            json!({ "id": "z", "name": "Paint" }),
        ];
        assert_eq!(ids(&search(&list(vs.clone()), "  NOTE ")), ["x", "isl"]);
        assert!(search(&list(vs.clone()), "   ").is_empty() && is_blank(" \u{FEFF}"));
        for i in 0..30 {
            vs.push(json!({ "id": format!("n{i}"), "name": "note" }));
        }
        assert_eq!(search(&list(vs), "note").len(), SEARCH_MAX);
    }

    #[test]
    fn recent_is_launched_apps_newest_first_capped_at_8() {
        let mut vs = vec![
            json!({ "id": "a", "kind": "app", "lastLaunched": 5 }),
            json!({ "id": "b", "kind": "app", "lastLaunched": null }),
            json!({ "id": "i", "kind": "island", "lastLaunched": 9 }),
            json!({ "id": "c", "kind": "app", "lastLaunched": 7 }),
        ];
        assert_eq!(ids(&recent(&list(vs.clone()))), ["c", "a"]);
        for i in 0..10 {
            vs.push(json!({ "id": format!("n{i}"), "kind": "app", "lastLaunched": 100 + i }));
        }
        let r = list(vs);
        let got = recent(&r);
        assert_eq!(got.len(), RECENT_MAX);
        assert_eq!(got[0].str("id"), Some("n9"));
    }

    #[test]
    fn path_label_walks_up_the_islands() {
        let apps = list(vec![
            json!({ "id": "w", "kind": "island", "name": "Work", "parentId": null }),
            json!({ "id": "t", "kind": "island", "name": "Tools", "parentId": "w" }),
            json!({ "id": "p", "kind": "island", "name": "P", "parentId": "q" }),
            json!({ "id": "q", "kind": "island", "name": "Q", "parentId": "p" }),
        ]);
        assert_eq!(path_label(&apps, None), "Home");
        assert_eq!(path_label(&apps, Some("gone")), "Home");
        assert_eq!(path_label(&apps, Some("t")), "Work / Tools");
        assert!(!path_label(&apps, Some("p")).is_empty()); // a cycle ends
    }
}
