//! Spatial focus (renderer/media/focus.js): the arrow keys and a pad's D-pad
//! move to the nearest element in that direction, by the rectangles the
//! window reports.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub fn parse(s: &str) -> Option<Dir> {
        Some(match s {
            "up" => Dir::Up,
            "down" => Dir::Down,
            "left" => Dir::Left,
            "right" => Dir::Right,
            _ => return None,
        })
    }
}

// Only elements past the current one's edge count, with a little slack for
// the focus ring: otherwise a wide button's centre could sit "right of" a
// narrow one above it.
const EDGE_SLACK: f32 = 8.0;

fn beyond(from: Rect, to: Rect, dir: Dir) -> bool {
    match dir {
        Dir::Right => to.x >= from.right() - EDGE_SLACK,
        Dir::Left => to.right() <= from.x + EDGE_SLACK,
        Dir::Down => to.y >= from.bottom() - EDGE_SLACK,
        Dir::Up => to.bottom() <= from.y + EDGE_SLACK,
    }
}

/// Distance along the direction plus twice the drift sideways, so moving
/// down a column prefers the tile right below over a closer one to the side.
fn score(from: Rect, to: Rect, dir: Dir) -> f32 {
    if !beyond(from, to, dir) {
        return f32::INFINITY;
    }
    let dx = (to.x + to.w / 2.0) - (from.x + from.w / 2.0);
    let dy = (to.y + to.h / 2.0) - (from.y + from.h / 2.0);
    let (along, across) = match dir {
        Dir::Left => (-dx, dy.abs()),
        Dir::Right => (dx, dy.abs()),
        Dir::Up => (-dy, dx.abs()),
        Dir::Down => (dy, dx.abs()),
    };
    if along <= 1.0 {
        return f32::INFINITY;
    }
    along + across * 2.0
}

fn nearest<'a>(items: impl Iterator<Item = (&'a str, Rect)>, current: &str, from: Rect, dir: Dir) -> Option<&'a str> {
    let mut best = None;
    let mut best_score = f32::INFINITY;
    for (id, r) in items {
        if id == current {
            continue;
        }
        let s = score(from, r, dir);
        if s < best_score {
            best = Some(id);
            best_score = s;
        }
    }
    best
}

/// The element to move to from `current`. The area the focus is in (its
/// screen, or the top bar: `area` says which an id belongs to) is tried
/// first and the rest only when nothing in it lies that way, so Right from
/// Settings' section list goes into the section, not up to the tabs.
pub fn step<'a>(items: &[(&'a str, Rect)], current: &str, from: Rect, dir: Dir, area: impl Fn(&str) -> u8) -> Option<&'a str> {
    let here = area(current);
    nearest(items.iter().copied().filter(|(id, _)| area(id) == here), current, from, dir)
        .or_else(|| nearest(items.iter().copied(), current, from, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn top(id: &str) -> u8 {
        u8::from(id.starts_with("tab:"))
    }

    #[test]
    fn down_a_column_prefers_the_tile_right_below_over_a_closer_one_aside() {
        let items = [("a", r(0.0, 0.0, 100.0, 100.0)), ("below", r(0.0, 300.0, 100.0, 100.0)), ("aside", r(160.0, 130.0, 100.0, 100.0))];
        assert_eq!(step(&items, "a", items[0].1, Dir::Down, |_| 0), Some("below"));
    }

    #[test]
    fn nothing_that_way_is_none_and_the_current_one_never_counts() {
        let items = [("a", r(0.0, 0.0, 100.0, 50.0)), ("b", r(200.0, 0.0, 100.0, 50.0))];
        assert_eq!(step(&items, "a", items[0].1, Dir::Left, |_| 0), None);
        assert_eq!(step(&items, "a", items[0].1, Dir::Right, |_| 0), Some("b"));
        assert_eq!(step(&items, "b", items[1].1, Dir::Right, |_| 0), None);
    }

    #[test]
    fn only_elements_past_the_edge_count() {
        // A wide button below a narrow one: its centre is to the right, but it isn't "right of" it.
        let items = [("narrow", r(0.0, 0.0, 100.0, 40.0)), ("wide", r(0.0, 60.0, 600.0, 40.0))];
        assert_eq!(step(&items, "narrow", items[0].1, Dir::Right, |_| 0), None);
    }

    #[test]
    fn the_own_area_is_tried_first_then_everything() {
        let items = [
            ("tab:games", r(300.0, 0.0, 100.0, 40.0)),
            ("sec:a", r(0.0, 100.0, 200.0, 40.0)),
            ("field", r(260.0, 300.0, 300.0, 40.0)),
        ];
        // Right from the section list: the field in the section, though the tab scores closer.
        assert_eq!(step(&items, "sec:a", items[1].1, Dir::Right, top), Some("field"));
        // Up from the section list: nothing in the screen that way, so the top bar.
        assert_eq!(step(&items, "sec:a", items[1].1, Dir::Up, top), Some("tab:games"));
    }
}
