//! Grid-cell footprint math for the freeform, snap-to-grid canvas used at
//! every level (Home, and inside each island). Loose apps and minimized
//! islands take one cell; expanded islands take a user-resizable block
//! (defaulting to 3x2). Positions/sizes are integer cell coordinates, not
//! pixels, and every item's position is scoped to its own parent - siblings
//! inside the same island form their own independent coordinate space.
//!
//! Cells are `f64` because they are JS numbers in apps.json: a hand-edited
//! fractional cell has to behave here as it does in 1.2.2.
use serde_json::Value;

use crate::store::{parent_key, strict_eq, Entry};

pub const EXPANDED_ISLAND_W: f64 = 3.0;
pub const EXPANDED_ISLAND_H: f64 = 2.0;
pub const MIN_ISLAND_W: f64 = 2.0;
pub const MIN_ISLAND_H: f64 = 2.0;
pub const MAX_ISLAND_W: f64 = 8.0;
pub const MAX_ISLAND_H: f64 = 6.0;

/// `find_first_free_cell`'s `maxCols` default in the JS.
pub const DEFAULT_MAX_COLS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn footprint_of(entry: &Entry) -> Size {
    if entry.str("kind") == Some("island") && entry.get("collapsed") == Some(&Value::Bool(false)) {
        // `entry.w || 3`: a missing, zero or non-number size falls back.
        let side = |key, fallback| entry.num(key).filter(|n| *n != 0.0).unwrap_or(fallback);
        return Size { w: side("w", EXPANDED_ISLAND_W), h: side("h", EXPANDED_ISLAND_H) };
    }
    Size { w: 1.0, h: 1.0 }
}

pub fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

fn is_free(occupied: &[Rect], x: f64, y: f64, w: f64, h: f64) -> bool {
    if x < 0.0 || y < 0.0 {
        return false;
    }
    let rect = Rect { x, y, w, h };
    !occupied.iter().any(|o| rects_overlap(&rect, o))
}

/// First free cell scanning top-left to bottom-right, reading order. Used for
/// brand-new items and one-time migration so placement is deterministic.
pub fn find_first_free_cell(occupied: &[Rect], w: f64, h: f64, max_cols: u32) -> Cell {
    for y in 0..10000 {
        for x in 0..max_cols {
            let (x, y) = (f64::from(x), f64::from(y));
            if is_free(occupied, x, y, w, h) {
                return Cell { x, y };
            }
        }
    }
    Cell { x: 0.0, y: 0.0 }
}

/// Nearest free cell to a drop target, spiraling outward ring by ring. Used
/// when the user drags something to a spot that's already occupied.
pub fn find_nearest_free_cell(occupied: &[Rect], target_x: f64, target_y: f64, w: f64, h: f64) -> Cell {
    let tx = target_x.max(0.0);
    let ty = target_y.max(0.0);
    if is_free(occupied, tx, ty, w, h) {
        return Cell { x: tx, y: ty };
    }

    for radius in 1..=200i32 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue; // ring edge only
                }
                let x = (tx + f64::from(dx)).max(0.0);
                let y = (ty + f64::from(dy)).max(0.0);
                if is_free(occupied, x, y, w, h) {
                    return Cell { x, y };
                }
            }
        }
    }
    find_first_free_cell(occupied, w, h, DEFAULT_MAX_COLS)
}

/// Cell footprints occupied among the siblings sharing `parent_id` (None or
/// null = Home, or an island's id), leaving out `exclude_id` (usually the
/// item being placed). Built fresh on every call: a cached list is how a
/// batch ends up stacking two items on one cell.
pub fn sibling_footprints(apps: &[Entry], parent_id: Option<&Value>, exclude_id: Option<&Value>) -> Vec<Rect> {
    let key = parent_id.filter(|v| crate::store::truthy(Some(v)));
    apps.iter()
        .filter(|a| strict_eq(parent_key(a), key) && !strict_eq(a.get("id"), exclude_id))
        .filter_map(|a| {
            let (x, y) = (a.num("gridX")?, a.num("gridY")?);
            let fp = footprint_of(a);
            Some(Rect { x, y, w: fp.w, h: fp.h })
        })
        .collect()
}

fn free_cell_for(apps: &[Entry], entry: &Entry, target: Option<(f64, f64)>) -> Cell {
    let fp = footprint_of(entry);
    let occupied = sibling_footprints(apps, entry.get("parentId"), entry.get("id"));
    match target {
        // Math.round: halves go up, not away from zero.
        Some((x, y)) if x.is_finite() && y.is_finite() => {
            find_nearest_free_cell(&occupied, (x + 0.5).floor(), (y + 0.5).floor(), fp.w, fp.h)
        }
        _ => find_first_free_cell(&occupied, fp.w, fp.h, DEFAULT_MAX_COLS),
    }
}

/// Places `entry` (already carrying its parentId) on a free cell among its
/// siblings in `apps`: the nearest one to `target` when one is given, the
/// first one in reading order otherwise. Writes gridX/gridY onto the entry,
/// so the next placement against the same `apps` sees it.
///
/// For an entry that is not in `apps` yet; `place_free_at` is the same for
/// one that is.
pub fn place_free(apps: &[Entry], entry: &mut Entry, target: Option<(f64, f64)>) -> Cell {
    let pos = free_cell_for(apps, entry, target);
    entry.set_num("gridX", pos.x);
    entry.set_num("gridY", pos.y);
    pos
}

/// `place_free` for `apps[index]`.
pub fn place_free_at(apps: &mut [Entry], index: usize, target: Option<(f64, f64)>) -> Cell {
    let pos = free_cell_for(apps, &apps[index], target);
    apps[index].set_num("gridX", pos.x);
    apps[index].set_num("gridY", pos.y);
    pos
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cell(x: f64, y: f64) -> Rect {
        rect(x, y, 1.0, 1.0)
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn at(x: f64, y: f64) -> Cell {
        Cell { x, y }
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
    }

    fn entry(v: Value) -> Entry {
        Entry::from_value(v).unwrap()
    }

    fn grid(e: &Entry) -> (f64, f64) {
        (e.num("gridX").unwrap(), e.num("gridY").unwrap())
    }

    // Small seeded PRNG (mulberry32) so the randomized cases are repeatable.
    struct Rng(u32);

    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x6d2b_79f5);
            let seed = self.0;
            let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
            t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
            f64::from(t ^ (t >> 14)) / 4_294_967_296.0
        }

        fn below(&mut self, n: f64) -> f64 {
            (self.next() * n).floor()
        }
    }

    fn random_occupied(rand: &mut Rng, count: usize, span: f64) -> Vec<Rect> {
        (0..count)
            .map(|_| {
                let x = rand.below(span);
                let y = rand.below(span);
                let w = 1.0 + rand.below(3.0);
                let h = 1.0 + rand.below(2.0);
                rect(x, y, w, h)
            })
            .collect()
    }

    #[test]
    fn footprint_of_apps_and_collapsed_islands_are_1x1_expanded_islands_use_their_size() {
        let one = Size { w: 1.0, h: 1.0 };
        assert_eq!(footprint_of(&entry(json!({ "kind": "app" }))), one);
        assert_eq!(footprint_of(&entry(json!({ "kind": "island", "collapsed": true, "w": 5, "h": 4 }))), one);
        assert_eq!(footprint_of(&entry(json!({ "kind": "island" }))), one, "collapsed is the default");
        assert_eq!(
            footprint_of(&entry(json!({ "kind": "island", "collapsed": false, "w": 5, "h": 4 }))),
            Size { w: 5.0, h: 4.0 }
        );
        assert_eq!(
            footprint_of(&entry(json!({ "kind": "island", "collapsed": false }))),
            Size { w: EXPANDED_ISLAND_W, h: EXPANDED_ISLAND_H }
        );
    }

    #[test]
    fn find_first_free_cell_fills_in_reading_order_and_wraps_after_max_cols() {
        assert_eq!(find_first_free_cell(&[], 1.0, 1.0, DEFAULT_MAX_COLS), at(0.0, 0.0));
        assert_eq!(find_first_free_cell(&[cell(0.0, 0.0), cell(1.0, 0.0)], 1.0, 1.0, DEFAULT_MAX_COLS), at(2.0, 0.0));
        assert_eq!(
            find_first_free_cell(&[cell(0.0, 0.0), cell(2.0, 0.0)], 1.0, 1.0, DEFAULT_MAX_COLS),
            at(1.0, 0.0),
            "fills gaps"
        );

        let full_row: Vec<Rect> = (0..8).map(|x| cell(f64::from(x), 0.0)).collect();
        assert_eq!(find_first_free_cell(&full_row, 1.0, 1.0, DEFAULT_MAX_COLS), at(0.0, 1.0));
        assert_eq!(
            find_first_free_cell(&[cell(0.0, 0.0), cell(1.0, 0.0), cell(2.0, 0.0)], 1.0, 1.0, 3),
            at(0.0, 1.0)
        );
    }

    #[test]
    fn find_first_free_cell_finds_room_for_a_whole_multi_cell_footprint() {
        // A 2x2 can't start at (0,0) because (1,1) is taken, nor at (1,0) or (2,0).
        assert_eq!(find_first_free_cell(&[cell(1.0, 1.0)], 2.0, 2.0, DEFAULT_MAX_COLS), at(2.0, 0.0));
        // Something big in the way: the next spot is beside it, not on top of it.
        assert_eq!(find_first_free_cell(&[rect(0.0, 0.0, 3.0, 2.0)], 3.0, 2.0, DEFAULT_MAX_COLS), at(3.0, 0.0));
    }

    #[test]
    fn find_first_free_cell_never_overlaps_what_is_already_placed() {
        let mut rand = Rng(1);
        let mut placed: Vec<Rect> = Vec::new();
        for i in 0..200 {
            let w = 1.0 + rand.below(3.0);
            let h = 1.0 + rand.below(2.0);
            let pos = find_first_free_cell(&placed, w, h, DEFAULT_MAX_COLS);
            let r = rect(pos.x, pos.y, w, h);
            assert!(!placed.iter().any(|p| overlaps(p, &r)), "item {i} overlaps");
            placed.push(r);
        }
    }

    #[test]
    fn find_nearest_free_cell_keeps_a_free_drop_target_and_clamps_negative_targets() {
        assert_eq!(find_nearest_free_cell(&[cell(0.0, 0.0)], 4.0, 3.0, 1.0, 1.0), at(4.0, 3.0));
        assert_eq!(find_nearest_free_cell(&[], -3.0, -1.0, 1.0, 1.0), at(0.0, 0.0));
    }

    #[test]
    fn find_nearest_free_cell_moves_an_occupied_drop_to_an_adjacent_free_cell() {
        let pos = find_nearest_free_cell(&[cell(5.0, 5.0)], 5.0, 5.0, 1.0, 1.0);
        assert_eq!((pos.x - 5.0).abs().max((pos.y - 5.0).abs()), 1.0);
    }

    #[test]
    fn find_nearest_free_cell_returns_a_non_overlapping_cell_at_the_smallest_possible_distance() {
        let mut rand = Rng(7);
        for round in 0..300 {
            let occupied = random_occupied(&mut rand, 25, 10.0);
            let w = 1.0 + rand.below(3.0);
            let h = 1.0 + rand.below(2.0);
            let tx = rand.below(12.0) - 1.0;
            let ty = rand.below(12.0) - 1.0;

            let pos = find_nearest_free_cell(&occupied, tx, ty, w, h);
            let r = rect(pos.x, pos.y, w, h);
            assert!(pos.x >= 0.0 && pos.y >= 0.0, "never negative");
            assert!(!occupied.iter().any(|o| overlaps(o, &r)), "round {round}: overlaps");

            // Brute-force the closest free spot (Chebyshev distance from the
            // clamped target) and check nothing nearer was skipped.
            let cx = tx.max(0.0);
            let cy = ty.max(0.0);
            let dist = |x: f64, y: f64| (x - cx).abs().max((y - cy).abs());
            let mut best = f64::INFINITY;
            for y in 0..20 {
                for x in 0..20 {
                    let (x, y) = (f64::from(x), f64::from(y));
                    let r = rect(x, y, w, h);
                    if !occupied.iter().any(|o| overlaps(o, &r)) {
                        best = best.min(dist(x, y));
                    }
                }
            }
            assert_eq!(dist(pos.x, pos.y), best, "round {round}: a nearer free cell existed");
        }
    }

    fn entry_at(id: &str, x: f64, y: f64) -> Entry {
        entry(json!({ "id": id, "kind": "app", "parentId": null, "gridX": x, "gridY": y }))
    }

    // entryAt(id) with no cell: gridX/gridY undefined in the JS.
    fn entry_unplaced(id: &str) -> Entry {
        entry(json!({ "id": id, "kind": "app", "parentId": null }))
    }

    fn unit(e: &Entry) -> Rect {
        let (x, y) = grid(e);
        cell(x, y)
    }

    #[test]
    fn place_free_two_items_forced_onto_the_same_cell_in_one_batch_end_on_two_different_cells() {
        let mut apps = vec![entry_at("a", 0.0, 0.0), entry_at("b", 1.0, 0.0), entry_at("c", 5.0, 5.0)];
        place_free_at(&mut apps, 1, Some((3.0, 3.0)));
        place_free_at(&mut apps, 2, Some((3.0, 3.0)));
        assert_eq!(grid(&apps[1]), (3.0, 3.0));
        assert_ne!(grid(&apps[2]), (3.0, 3.0));
        let rects: Vec<Rect> = apps.iter().map(unit).collect();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(!overlaps(&rects[i], &rects[j]));
            }
        }
    }

    #[test]
    fn place_free_forced_onto_an_occupied_cell_lands_one_ring_out_no_target_means_the_first_free_cell() {
        let mut apps = vec![entry_at("a", 0.0, 0.0), entry_at("b", 1.0, 0.0), entry_unplaced("n")];
        place_free_at(&mut apps, 0, Some((1.0, 0.0)));
        let (x, y) = grid(&apps[0]);
        assert!(!(x == 1.0 && y == 0.0));
        assert_eq!((x - 1.0).abs().max(y.abs()), 1.0);
        place_free_at(&mut apps, 2, None);
        for other in &apps[..2] {
            assert!(!overlaps(&unit(&apps[2]), &unit(other)));
        }
    }

    #[test]
    fn place_free_only_siblings_count_and_an_expanded_island_blocks_its_whole_footprint() {
        let mut child = entry_at("child", 0.0, 0.0);
        child.set("parentId", "i");
        let mut apps = vec![
            entry(json!({ "id": "i", "kind": "island", "parentId": null, "collapsed": false, "w": 3, "h": 2, "gridX": 0, "gridY": 0 })),
            child,
            entry_unplaced("x"),
        ];
        place_free_at(&mut apps, 2, Some((1.0, 1.0)));
        assert!(!overlaps(&unit(&apps[2]), &rect(0.0, 0.0, 3.0, 2.0)));
        place_free_at(&mut apps, 1, Some((0.0, 0.0))); // the island's own space is empty
        assert_eq!(grid(&apps[1]), (0.0, 0.0));
    }

    // Not in the JS tests: the port splits placeFree in two.
    #[test]
    fn place_free_for_an_entry_outside_the_list_writes_whole_numbers() {
        let apps = vec![entry_at("a", 0.0, 0.0)];
        let mut new = entry_unplaced("n");
        assert_eq!(place_free(&apps, &mut new, Some((0.4, -0.5))), at(1.0, 0.0));
        assert_eq!(serde_json::to_string(&new).unwrap(), r#"{"id":"n","kind":"app","parentId":null,"gridX":1,"gridY":0}"#);
    }
}
