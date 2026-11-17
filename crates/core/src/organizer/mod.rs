//! The window organizer's logic: layouts and zones, the drag strip, panel
//! auto-hide, the window picker, scenes, fullscreen detection and the cell
//! math of the layout editor and grid placer. Ported from `lib/zones.js`,
//! `layouts.js`, `strip.js`, `autohide.js`, `picker.js`, `scenes.js`,
//! `fullscreen.js` and `renderer/cells.js`.
//!
//! Screen rects and points are `f64`, as they were JS numbers: every sum and
//! rounding then comes out as it did there.
pub mod autohide;
pub mod cells;
pub mod fullscreen;
pub mod layouts;
pub mod picker;
pub mod scenes;
pub mod strip;
pub mod zones;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect { x, y, width, height }
    }

    /// Left and top edges are inside, right and bottom are not.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }
}

/// An open top-level window, as the window manager lists them (top of the
/// z-order first).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenWindow {
    pub hwnd: i64,
    pub exe: String,
    pub title: String,
    pub minimized: bool,
}

pub(crate) use crate::js::{js_num, js_round, js_trim, utf16_len, utf16_prefix, whole_number};
