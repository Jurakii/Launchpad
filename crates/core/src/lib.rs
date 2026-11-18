//! Launchpad's logic with no Windows or UI code in it, so `cargo test` runs
//! in seconds. Ported module for module from the Electron app's `lib/`.
//!
//! Conventions:
//! - Nothing here knows where the data folder is. Functions that touch disk
//!   take `dir: &Path` (what Electron called userData); state the JS kept in
//!   module variables lives in a struct.
//! - Files on disk keep the 1.2.2 shape, unknown keys included, so both
//!   builds can read each other's data.
//! - Anything that needs the OS (image decoding, starting a process, HTTP)
//!   comes in as a trait or closure.

mod error;
pub(crate) mod js;
pub mod ids;
pub mod safe_file;
#[cfg(test)]
pub(crate) mod test_util;
pub mod time;

pub use error::{Error, Result};
pub use js::truthy;

pub mod backup;
pub mod defaults;
pub mod file_store;
pub mod items;
pub mod layout;
pub mod reset;
pub mod settings;
pub mod store;
pub mod taskbar;

pub mod organizer;

pub mod media;

pub mod favicon;
pub mod update;
pub mod ui_prefs;
