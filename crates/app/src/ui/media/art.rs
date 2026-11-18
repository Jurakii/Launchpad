//! Library art for the window, decoded through WIC at the size it's drawn
//! (RUST_PORT_PLAN.md §4.3), on a worker thread so a full library doesn't
//! hold up the UI. What's asked for and not yet decoded comes back through
//! `post`, and Media draws again.
use crate::ui::images::to_image;
use crate::win::image::{Rgba, Wic};
use slint::Image;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Above this the cache starts over (tiles are ~100 KB each).
const CAP_BYTES: usize = 96 << 20;

/// A file at one size: `cover` fills the box (cropped), otherwise it fits inside.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub path: PathBuf,
    /// The slot's updatedAt, so new art is a new key.
    pub version: i64,
    pub w: u32,
    pub h: u32,
    pub cover: bool,
}

#[derive(Default)]
pub struct Cache {
    images: HashMap<Key, Image>,
    bytes: usize,
    wanted: HashSet<Key>,
    /// Keys being decoded now.
    busy: bool,
    failed: HashSet<Key>,
}

impl Cache {
    /// The image if it's ready; otherwise it's queued (see `take_jobs`).
    pub fn get(&mut self, key: Key) -> Option<Image> {
        if let Some(i) = self.images.get(&key) {
            return Some(i.clone());
        }
        if !self.failed.contains(&key) {
            self.wanted.insert(key);
        }
        None
    }

    /// What to decode next, if nothing is being decoded.
    pub fn take_jobs(&mut self) -> Option<Vec<Key>> {
        if self.busy || self.wanted.is_empty() {
            return None;
        }
        self.busy = true;
        Some(self.wanted.drain().collect())
    }

    pub fn done(&mut self, results: Vec<(Key, Option<Rgba>)>) {
        self.busy = false;
        for (key, rgba) in results {
            match rgba {
                Some(rgba) => {
                    let n = rgba.rgba.len();
                    if self.bytes + n > CAP_BYTES {
                        self.images.clear();
                        self.bytes = 0;
                    }
                    self.bytes += n;
                    self.images.insert(key, to_image(rgba));
                }
                None => {
                    self.failed.insert(key);
                }
            }
        }
    }
}

/// Decodes `jobs` (on a worker thread).
pub fn decode(jobs: Vec<Key>) -> Vec<(Key, Option<Rgba>)> {
    let Ok(wic) = Wic::new() else { return jobs.into_iter().map(|k| (k, None)).collect() };
    jobs.into_iter()
        .map(|k| {
            let img = std::fs::read(&k.path).ok().and_then(|bytes| {
                wic.decode_rgba(&bytes, |w, h| {
                    let (sx, sy) = (k.w as f64 / w.max(1) as f64, k.h as f64 / h.max(1) as f64);
                    let s = if k.cover { sx.max(sy) } else { sx.min(sy) }.min(1.0);
                    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
                })
                .ok()
            });
            (k, img)
        })
        .collect()
}
