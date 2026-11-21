//! Media: the games and videos library, its players and emulators, folder
//! scans, tracked launches, controller input and colours. Ported from
//! `lib/media/` and the pure parts of `renderer/media/`.

pub mod art;
pub mod backup;
pub mod controller;
pub mod import;
pub mod launch;
pub mod library;
pub mod pad;
pub mod players;
pub mod scan;
pub mod theme;
mod win_path;

pub(crate) use crate::js::{js_is_space, js_trim, truthy};

/// `/<suffix>$/i` for an ASCII suffix.
pub(crate) fn ends_with_ignore_case(s: &str, suffix: &str) -> bool {
    let (s, suffix) = (s.as_bytes(), suffix.as_bytes());
    s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

#[cfg(test)]
pub(crate) fn test_entry(v: serde_json::Value) -> library::Entry {
    library::Entry(v.as_object().unwrap().clone())
}
