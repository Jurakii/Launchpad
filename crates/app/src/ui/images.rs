//! Images for the UI, decoded through WIC at the size they're drawn
//! (RUST_PORT_PLAN.md §4.3): icons at tile size, the wallpaper at screen size.
//! Cached while the window exists; `clear` drops them all when it hides.
use crate::win::display;
use crate::win::image::{Rgba, Wic};
use launchpad_core::Result;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Above this the cache starts over; a full grid of icons is a few MB.
const CACHE_CAP_BYTES: usize = 32 << 20;

pub struct Images {
    pub wic: Wic,
    cache: RefCell<HashMap<(PathBuf, u32), Image>>,
    bytes: Cell<usize>,
}

pub fn to_image(img: Rgba) -> Image {
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&img.rgba, img.width, img.height);
    Image::from_rgba8(buf)
}

impl Images {
    pub fn new() -> Result<Images> {
        Ok(Images { wic: Wic::new()?, cache: RefCell::new(HashMap::new()), bytes: Cell::new(0) })
    }

    /// An image file (PNG, JPEG, ICO…) fitted inside `size`×`size` physical
    /// pixels, never enlarged. None if it's missing or won't decode.
    pub fn icon(&self, path: &Path, size: u32) -> Option<Image> {
        let key = (path.to_path_buf(), size);
        if let Some(img) = self.cache.borrow().get(&key) {
            return Some(img.clone());
        }
        let bytes = std::fs::read(path).ok()?;
        let rgba = self
            .wic
            .decode_rgba(&bytes, |w, h| {
                let s = (size as f64 / w.max(h).max(1) as f64).min(1.0);
                (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
            })
            .ok()?;
        Some(self.remember(key, rgba))
    }

    /// The wallpaper, scaled down to just cover the work area.
    pub fn wallpaper(&self, path: &Path) -> Option<Image> {
        let wa = display::work_area();
        let (sw, sh) = ((wa.right - wa.left).max(1) as f64, (wa.bottom - wa.top).max(1) as f64);
        let key = (path.to_path_buf(), 0);
        if let Some(img) = self.cache.borrow().get(&key) {
            return Some(img.clone());
        }
        let bytes = std::fs::read(path).ok()?;
        let rgba = self
            .wic
            .decode_rgba(&bytes, |w, h| {
                let s = (sw / w as f64).max(sh / h as f64).min(1.0);
                (((w as f64 * s).ceil() as u32).max(1), ((h as f64 * s).ceil() as u32).max(1))
            })
            .ok()?;
        Some(self.remember(key, rgba))
    }

    fn remember(&self, key: (PathBuf, u32), rgba: Rgba) -> Image {
        let n = rgba.rgba.len();
        if self.bytes.get() + n > CACHE_CAP_BYTES {
            self.clear();
        }
        let img = to_image(rgba);
        self.bytes.set(self.bytes.get() + n);
        self.cache.borrow_mut().insert(key, img.clone());
        img
    }

    /// Forgets one file (it changed on disk), every size of it.
    pub fn forget(&self, path: &Path) {
        self.cache.borrow_mut().retain(|(p, _), _| p != path);
    }

    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
        self.bytes.set(0);
    }
}
