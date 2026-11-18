//! The Windows layer: everything that talks to Win32, COM or WinRT, so the
//! rest of the app (and `launchpad-core`) never does. Replaces the PowerShell
//! helpers and Electron's own APIs (RUST_PORT_PLAN.md §4.2).
//!
//! Conventions:
//! - Coordinates are physical pixels; the process is per-monitor DPI aware v2.
//! - Strings cross the API as UTF-16 (`wide` / `from_wide`), so non-ASCII
//!   names arrive intact (the old helpers garbled them).
//! - Errors are `launchpad_core::Error`; `.ctx("what failed")` turns a
//!   Windows error into a message.
//! - Each area module has `pub fn probe(args: &[String]) -> Result<Value>`
//!   for `--probe` (see main.rs).

pub mod appbar;
pub mod audio;
pub mod autostart;
pub mod bluetooth;
pub mod crypto;
pub mod dialogs;
pub mod display;
pub mod drives;
pub mod hotkey;
pub mod http;
pub mod image;
pub mod instance;
pub mod launch;
pub mod notify;
pub mod power;
pub mod services;
pub mod tray;
pub mod wifi;
pub mod windowmgr;

use launchpad_core::{Error, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

/// Once, first thing in `main`.
pub fn init_process() {
    // Fails harmlessly if a manifest already set it.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// NUL-terminated UTF-16 for an API argument.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

/// UTF-16 from an API, up to the first NUL; unpaired surrogates become U+FFFD.
pub fn from_wide(s: &[u16]) -> String {
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..end])
}

/// COM for the current thread, released on drop. Apartment-threaded, which
/// the shell objects (IShellLink, file dialogs, image factory) need.
pub struct Com(bool);

impl Com {
    pub fn init() -> Com {
        // S_FALSE (already initialised) still needs a matching uninit;
        // RPC_E_CHANGED_MODE (another model on this thread) must not get one.
        Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub trait Context<T> {
    fn ctx(self, what: &str) -> Result<T>;
}

impl<T> Context<T> for windows::core::Result<T> {
    fn ctx(self, what: &str) -> Result<T> {
        self.map_err(|e| Error::msg(format!("{what}: {}", e.message())))
    }
}

/// The probe argument after `name`, e.g. `arg(args, "--id")`.
pub fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// Actions that change the machine refuse to run without `--yes`.
pub fn require_yes(args: &[String], what: &str) -> Result<()> {
    if args.iter().any(|a| a == "--yes") {
        Ok(())
    } else {
        Err(Error::msg(format!("{what} changes this PC; add --yes to really do it.")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_round_trips_non_ascii() {
        let w = wide("Café 日本 🎮");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(from_wide(&w), "Café 日本 🎮");
        assert_eq!(from_wide(&[0x61, 0xD800, 0x62]), "a\u{FFFD}b");
    }

    #[test]
    fn arg_reads_the_value_after_a_flag() {
        let a: Vec<String> = ["list", "--id", "x"].map(String::from).to_vec();
        assert_eq!(arg(&a, "--id"), Some("x"));
        assert_eq!(arg(&a, "--name"), None);
        assert!(require_yes(&a, "Sleep").is_err());
    }
}
