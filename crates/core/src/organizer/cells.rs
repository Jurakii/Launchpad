//! Cell math for the window organizer's pages: the layout editor in the main
//! window and the grid placer. A block is a rectangle of whole cells
//! { x, y, w, h } on a cols x rows grid, the same model as a zone in
//! `zones.rs`.
use super::zones::{Layout, Zone};
use super::{js_round, js_trim, Rect};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Block {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl From<&Zone> for Block {
    fn from(z: &Zone) -> Block {
        Block { x: z.x, y: z.y, w: z.w, h: z.h }
    }
}

fn clamp_cells(n: i32, min: i32, max: i32) -> i32 {
    max.min(min.max(n))
}
fn clamp_f(n: f64, min: f64, max: f64) -> f64 {
    max.min(min.max(n))
}
fn blocks_overlap(a: Block, b: Block) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}
fn block_inside(inner: Block, outer: Block) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.w <= outer.x + outer.w && inner.y + inner.h <= outer.y + outer.h
}
fn by_reading_order(a: &Zone, b: &Zone) -> Ordering {
    a.y.cmp(&b.y).then(a.x.cmp(&b.x))
}

// ---------- the grid placer ----------

/// The block between two corner cells (x, y), given in either order.
pub fn cell_span(a: (i32, i32), b: (i32, i32)) -> Block {
    Block { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs() + 1, h: (a.1 - b.1).abs() + 1 }
}

/// Arrow keys: one cell over, same size, never off the grid.
pub fn move_block(block: Block, dx: i32, dy: i32, cols: i32, rows: i32) -> Block {
    Block {
        x: clamp_cells(block.x + dx, 0, cols - block.w),
        y: clamp_cells(block.y + dy, 0, rows - block.h),
        ..block
    }
}

/// Shift+arrows: right and down grow the block, left and up shrink it. The
/// top-left corner stays put, as in gTile.
pub fn resize_block(block: Block, dw: i32, dh: i32, cols: i32, rows: i32) -> Block {
    Block {
        w: clamp_cells(block.w + dw, 1, cols - block.x),
        h: clamp_cells(block.h + dh, 1, rows - block.y),
        ..block
    }
}

/// The block closest to rect, a window's frame relative to the area the grid
/// covers (`area_width` x `area_height`): each edge moves to the nearest grid
/// line, and what's off the area is cut off first. None when less than half
/// a cell of the window is on the area (minimized, or on another display).
pub fn block_from_rect(rect: Rect, area_width: f64, area_height: f64, cols: i32, rows: i32) -> Option<Block> {
    let (c, r) = (f64::from(cols), f64::from(rows));
    let left = rect.x.max(0.0);
    let top = rect.y.max(0.0);
    let right = (rect.x + rect.width).min(area_width);
    let bottom = (rect.y + rect.height).min(area_height);
    if right - left < area_width / c / 2.0 || bottom - top < area_height / r / 2.0 {
        return None;
    }
    let x = clamp_f(js_round(left / area_width * c), 0.0, c - 1.0);
    let y = clamp_f(js_round(top / area_height * r), 0.0, r - 1.0);
    let x2 = clamp_f(js_round(right / area_width * c), x + 1.0, c);
    let y2 = clamp_f(js_round(bottom / area_height * r), y + 1.0, r);
    Some(Block { x: x as i32, y: y as i32, w: (x2 - x) as i32, h: (y2 - y) as i32 })
}

/// Where the selection starts when the window's own spot can't be used.
pub fn default_block(cols: i32, rows: i32) -> Block {
    let w = 1.max(js_round(f64::from(cols) / 2.0) as i32);
    let h = 1.max(js_round(f64::from(rows) / 2.0) as i32);
    Block { x: (cols - w).div_euclid(2), y: (rows - h).div_euclid(2), w, h }
}

// ---------- the layout editor ----------
// Every cell of a layout in the editor belongs to exactly one zone, so
// merging and splitting can never leave a hole or an overlap. Zones keep
// their ids through edits (the top-left part keeps it), since anything
// saved against a layout, like a scene, refers to its zones by id.

/// The smallest block containing `block` that doesn't cut through a zone.
pub fn expand_to_zones(zones: &[Zone], block: Block) -> Block {
    let mut b = block;
    let mut grew = true;
    while grew {
        grew = false;
        for z in zones.iter().map(Block::from) {
            if !blocks_overlap(z, b) || block_inside(z, b) {
                continue;
            }
            let x = b.x.min(z.x);
            let y = b.y.min(z.y);
            b = Block { x, y, w: (b.x + b.w).max(z.x + z.w) - x, h: (b.y + b.h).max(z.y + z.h) - y };
            grew = true;
        }
    }
    b
}

pub fn zones_in(zones: &[Zone], block: Block) -> Vec<&Zone> {
    zones.iter().filter(|z| blocks_overlap(Block::from(*z), block)).collect()
}

fn next_zone_id(zones: &[Zone]) -> String {
    // f64 like JS Number(); /\d/ is ASCII digits only.
    let max = zones
        .iter()
        .filter_map(|z| z.id.strip_prefix('z'))
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()))
        .filter_map(|digits| digits.parse::<f64>().ok())
        .fold(0.0, f64::max);
    format!("z{}", max + 1.0)
}

/// A layout of cols x rows single-cell zones, ready to be merged. It has no
/// id yet (""), as in the JS.
pub fn blank_layout(name: &str, cols: i32, rows: i32) -> Layout {
    let mut zones = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            zones.push(Zone::new(&format!("z{}", zones.len() + 1), "", x, y, 1, 1));
        }
    }
    Layout { name: name.into(), cols, rows, zones, ..Default::default() }
}

/// One zone in place of every zone the block touches (after widening it so
/// it cuts through none). The top-left zone's id and name carry over.
pub fn merge_zones(layout: &Layout, block: Block) -> Layout {
    let b = expand_to_zones(&layout.zones, block);
    let mut covered = zones_in(&layout.zones, b);
    covered.sort_by(|a, b| by_reading_order(a, b));
    if covered.len() < 2 {
        return layout.clone();
    }
    let merged = Zone { x: b.x, y: b.y, w: b.w, h: b.h, ..covered[0].clone() };
    let mut zones: Vec<Zone> = layout.zones.iter().filter(|z| !blocks_overlap(Block::from(*z), b)).cloned().collect();
    zones.push(merged);
    zones.sort_by(by_reading_order);
    Layout { zones, ..layout.clone() }
}

/// Every zone the block touches, back into single cells. Each zone's
/// top-left cell keeps its id and name.
pub fn split_zones(layout: &Layout, block: Block) -> Layout {
    let mut out = Vec::new();
    let mut taken = layout.zones.clone();
    for z in &layout.zones {
        if !blocks_overlap(Block::from(z), block) || z.w * z.h == 1 {
            out.push(z.clone());
            continue;
        }
        for y in z.y..z.y + z.h {
            for x in z.x..z.x + z.w {
                if x == z.x && y == z.y {
                    out.push(Zone { w: 1, h: 1, ..z.clone() });
                    continue;
                }
                let cell = Zone::new(&next_zone_id(&taken), "", x, y, 1, 1);
                taken.push(cell.clone());
                out.push(cell);
            }
        }
    }
    out.sort_by(by_reading_order);
    Layout { zones: out, ..layout.clone() }
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

fn pos_size(z: &mut Zone, axis: Axis) -> (&mut i32, &mut i32) {
    match axis {
        Axis::X => (&mut z.x, &mut z.w),
        Axis::Y => (&mut z.y, &mut z.h),
    }
}

/// A different number of columns or rows. When the new count is a multiple
/// of the old one (or the other way round, with every zone edge on a line
/// that survives), zones are scaled and nothing is lost. Otherwise zones are
/// cut at the new edge, and cells that end up in no zone become zones.
pub fn resize_grid(layout: &Layout, cols: i32, rows: i32) -> Layout {
    let axis = |mut zones: Vec<Zone>, from: i32, to: i32, axis: Axis| -> Vec<Zone> {
        if to.checked_rem(from) == Some(0) {
            let f = to / from;
            for z in &mut zones {
                let (pos, size) = pos_size(z, axis);
                *pos *= f;
                *size *= f;
            }
            return zones;
        }
        if from.checked_rem(to) == Some(0) {
            let f = from / to;
            if zones.iter_mut().all(|z| {
                let (pos, size) = pos_size(z, axis);
                *pos % f == 0 && *size % f == 0
            }) {
                for z in &mut zones {
                    let (pos, size) = pos_size(z, axis);
                    *pos /= f;
                    *size /= f;
                }
                return zones;
            }
        }
        zones.retain_mut(|z| *pos_size(z, axis).0 < to);
        for z in &mut zones {
            let (pos, size) = pos_size(z, axis);
            *size = (*size).min(to - *pos);
        }
        zones
    };
    let mut zones = axis(axis(layout.zones.clone(), layout.cols, cols, Axis::X), layout.rows, rows, Axis::Y);
    for y in 0..rows {
        for x in 0..cols {
            if !zones.iter().any(|z| block_inside(Block { x, y, w: 1, h: 1 }, Block::from(z))) {
                let id = next_zone_id(&zones);
                zones.push(Zone::new(&id, "", x, y, 1, 1));
            }
        }
    }
    zones.sort_by(by_reading_order);
    Layout { cols, rows, zones, ..layout.clone() }
}

// Zones the user hasn't named are called "Zone 1", "Zone 2"… in reading
// order when the layout is saved. The editor treats those names as blank
// again, so the numbers follow later merges and splits.
fn is_auto_zone_name(name: &str) -> bool {
    name.strip_prefix("Zone ").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

pub fn name_zones(layout: &Layout) -> Layout {
    let mut n = 0;
    let mut zones = layout.zones.clone();
    zones.sort_by(by_reading_order);
    for z in &mut zones {
        let trimmed = js_trim(&z.name);
        z.name = if !trimmed.is_empty() && !is_auto_zone_name(trimmed) {
            trimmed.to_owned()
        } else {
            n += 1;
            format!("Zone {n}")
        };
    }
    Layout { zones, ..layout.clone() }
}

pub fn unname_zones(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    for z in &mut out.zones {
        if is_auto_zone_name(&z.name) {
            z.name.clear();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organizer::layouts::built_in_layouts;
    use crate::organizer::zones::validate_layout;

    fn built_in(id: &str) -> Layout {
        built_in_layouts().into_iter().find(|l| l.id == id).unwrap()
    }

    // What the editor promises: a valid layout whose zones cover every cell
    // exactly once.
    fn assert_tiles(layout: &Layout) {
        let mut v = layout.to_value();
        if layout.id.is_empty() {
            v["id"] = "x".into();
        }
        if layout.name.is_empty() {
            v["name"] = "x".into();
        }
        assert_eq!(validate_layout(&v), None);
        let area: i32 = layout.zones.iter().map(|z| z.w * z.h).sum();
        assert_eq!(area, layout.cols * layout.rows, "no holes");
    }

    fn shape(layout: &Layout) -> Vec<(&str, i32, i32, i32, i32)> {
        layout.zones.iter().map(|z| (z.id.as_str(), z.x, z.y, z.w, z.h)).collect()
    }
    fn b(x: i32, y: i32, w: i32, h: i32) -> Block {
        Block { x, y, w, h }
    }
    fn r(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect::new(x, y, width, height)
    }

    #[test]
    fn cell_span_the_block_between_two_corners_in_either_order() {
        assert_eq!(cell_span((1, 2), (3, 0)), b(1, 0, 3, 3));
        assert_eq!(cell_span((4, 1), (4, 1)), b(4, 1, 1, 1));
    }

    #[test]
    fn move_block_and_resize_block_never_leave_the_grid() {
        let blk = b(4, 2, 2, 2);
        assert_eq!(move_block(blk, 1, 1, 6, 4), blk, "already in the corner");
        assert_eq!(move_block(blk, -1, 0, 6, 4), b(3, 2, 2, 2));
        assert_eq!(resize_block(blk, 1, 1, 6, 4), blk, "no room to grow");
        assert_eq!(resize_block(blk, -1, -1, 6, 4), b(4, 2, 1, 1));
        assert_eq!(resize_block(b(4, 2, 1, 1), -1, -1, 6, 4), b(4, 2, 1, 1), "one cell at least");
        assert_eq!(resize_block(b(0, 0, 1, 1), 1, 0, 6, 4), b(0, 0, 2, 1));
    }

    #[test]
    fn block_from_rect_snaps_a_window_to_the_nearest_grid_lines() {
        let (aw, ah) = (1200.0, 800.0); // 6 x 4 cells of 200
        assert_eq!(block_from_rect(r(0.0, 0.0, 1200.0, 800.0), aw, ah, 6, 4), Some(b(0, 0, 6, 4)), "maximized");
        assert_eq!(block_from_rect(r(190.0, 220.0, 430.0, 370.0), aw, ah, 6, 4), Some(b(1, 1, 2, 2)));
        assert_eq!(
            block_from_rect(r(-7.0, -7.0, 614.0, 814.0), aw, ah, 6, 4),
            Some(b(0, 0, 3, 4)),
            "invisible borders hang off the edge"
        );
        assert_eq!(block_from_rect(r(500.0, 300.0, 40.0, 40.0), aw, ah, 6, 4), None, "smaller than half a cell");
        assert_eq!(
            block_from_rect(r(520.0, 350.0, 120.0, 120.0), aw, ah, 6, 4),
            Some(b(3, 2, 1, 1)),
            "a small window still gets a cell"
        );
    }

    #[test]
    fn block_from_rect_null_for_a_window_that_is_off_the_area() {
        let (aw, ah) = (1200.0, 800.0);
        assert_eq!(block_from_rect(r(-32000.0, -32000.0, 160.0, 28.0), aw, ah, 6, 4), None, "minimized");
        assert_eq!(block_from_rect(r(-1920.0, 0.0, 1000.0, 700.0), aw, ah, 6, 4), None, "another display");
    }

    #[test]
    fn default_block_sits_in_the_middle_of_the_grid() {
        assert_eq!(default_block(6, 4), b(1, 1, 3, 2));
        assert_eq!(default_block(4, 6), b(1, 1, 2, 3));
        assert_eq!(default_block(1, 1), b(0, 0, 1, 1));
    }

    #[test]
    fn blank_layout_one_zone_per_cell() {
        let layout = blank_layout("New", 4, 2);
        assert_tiles(&layout);
        assert_eq!(layout.zones.len(), 8);
    }

    #[test]
    fn merge_zones_widens_the_selection_so_it_never_cuts_a_zone_in_two() {
        // quarters, then the top two merged: a wide top zone and two bottom ones
        let top = merge_zones(&built_in("quarters"), b(0, 0, 2, 1));
        assert_tiles(&top);
        assert_eq!(shape(&top), [("top-left", 0, 0, 2, 1), ("bottom-left", 0, 1, 1, 1), ("bottom-right", 1, 1, 1, 1)]);

        // Selecting just the left column cuts through the wide top zone, so it
        // takes the whole layout.
        assert_eq!(expand_to_zones(&top.zones, b(0, 0, 1, 2)), b(0, 0, 2, 2));
        let all = merge_zones(&top, b(0, 0, 1, 2));
        assert_tiles(&all);
        assert_eq!(shape(&all), [("top-left", 0, 0, 2, 2)]);
    }

    #[test]
    fn merge_zones_keeps_the_top_left_zone_and_leaves_a_single_zone_alone() {
        let thirds = built_in("thirds");
        let merged = merge_zones(&thirds, b(1, 0, 2, 1));
        assert_eq!(shape(&merged), [("left", 0, 0, 1, 1), ("center", 1, 0, 2, 1)]);
        assert_eq!(merged.zones[1].name, "Center");
        // JS checked it was the same object; an equal copy here.
        assert_eq!(merge_zones(&thirds, b(1, 0, 1, 1)), thirds);
    }

    #[test]
    fn split_zones_breaks_zones_back_into_cells_with_fresh_ids_for_the_new_ones() {
        let split = split_zones(&built_in("main-stack"), b(1, 1, 1, 1));
        assert_tiles(&split);
        assert_eq!(split.zones.len(), 8, "main became 6 cells; the stack zones are outside the selection");
        assert_eq!(split.zones.iter().find(|z| z.id == "main"), Some(&Zone::new("main", "Main", 0, 0, 1, 1)));
        let fresh: Vec<&Zone> = split
            .zones
            .iter()
            .filter(|z| z.id.strip_prefix('z').is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())))
            .collect();
        assert_eq!(fresh.iter().map(|z| z.id.as_str()).collect::<Vec<_>>(), ["z1", "z2", "z3", "z4", "z5"]);
        assert!(fresh.iter().all(|z| z.name.is_empty()));
    }

    #[test]
    fn resize_grid_scales_zones_when_the_new_count_is_a_multiple_and_cuts_them_otherwise() {
        let focus = built_in("focus"); // 3x1: main 2 wide, side 1 wide
        let doubled = resize_grid(&focus, 6, 2);
        assert_tiles(&doubled);
        assert_eq!(shape(&doubled), [("main", 0, 0, 4, 2), ("side", 4, 0, 2, 2)]);

        assert_eq!(shape(&resize_grid(&doubled, 3, 1)), shape(&focus), "and back again");

        let cut = resize_grid(&focus, 2, 1);
        assert_tiles(&cut);
        assert_eq!(shape(&cut), [("main", 0, 0, 2, 1)], "the side zone falls off the edge");

        let grown = resize_grid(&focus, 4, 1);
        assert_tiles(&grown);
        assert_eq!(shape(&grown), [("main", 0, 0, 2, 1), ("side", 2, 0, 1, 1), ("z1", 3, 0, 1, 1)], "a new cell fills the gap");
    }

    #[test]
    fn resize_grid_only_scales_down_when_every_zone_edge_survives() {
        let wide = Layout {
            name: "Wide".into(),
            cols: 4,
            rows: 1,
            zones: vec![Zone::new("a", "", 0, 0, 1, 1), Zone::new("b", "", 1, 0, 3, 1)],
            ..Default::default()
        };
        let halved = resize_grid(&wide, 2, 1);
        assert_tiles(&halved);
        assert_eq!(shape(&halved), [("a", 0, 0, 1, 1), ("b", 1, 0, 1, 1)], "cut, not scaled: 1 and 3 are not on the new lines");
    }

    #[test]
    fn name_zones_numbers_the_unnamed_zones_in_reading_order_unname_zones_undoes_it() {
        let layout = Layout {
            name: "L".into(),
            cols: 2,
            rows: 2,
            zones: vec![Zone::new("b", "", 1, 0, 1, 2), Zone::new("a", " Main ", 0, 0, 1, 1), Zone::new("c", "Zone 7", 0, 1, 1, 1)],
            ..Default::default()
        };
        let named = name_zones(&layout);
        let names: Vec<(&str, &str)> = named.zones.iter().map(|z| (z.id.as_str(), z.name.as_str())).collect();
        assert_eq!(names, [("a", "Main"), ("b", "Zone 1"), ("c", "Zone 2")]);
        assert_eq!(unname_zones(&named).zones.iter().map(|z| z.name.as_str()).collect::<Vec<_>>(), ["Main", "", ""]);
    }

    // Not in the JS tests: a scale-down that is all-or-nothing must leave
    // the zones untouched when it gives up.
    #[test]
    fn resize_grid_does_not_half_scale_when_a_later_zone_is_off_the_lines() {
        let layout = Layout {
            name: "L".into(),
            cols: 4,
            rows: 1,
            zones: vec![Zone::new("a", "", 0, 0, 2, 1), Zone::new("b", "", 2, 0, 1, 1), Zone::new("c", "", 3, 0, 1, 1)],
            ..Default::default()
        };
        assert_eq!(shape(&resize_grid(&layout, 2, 1)), [("a", 0, 0, 2, 1)]);
    }
}
