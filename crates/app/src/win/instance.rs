//! Single instance (Electron's `requestSingleInstanceLock` / `second-instance`).
//! A named mutex per data folder: the default folder is per user, and a test
//! copy with its own `--user-data-dir` runs beside the real one. The first
//! instance names its `tray::MsgWindow` after the same key, so a second
//! launch finds it, asks it to show itself (`Event::ShowRequest`) and exits.
use super::tray::{CLASS, WM_SHOW_REQUEST};
use super::{wide, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, PostMessageW};

/// `--user-data-dir=X` or `--user-data-dir X`, as Electron takes it.
pub fn user_data_dir(args: &[String]) -> Option<PathBuf> {
    args.iter().enumerate().find_map(|(i, a)| match a.strip_prefix("--user-data-dir") {
        Some(v) if v.starts_with('=') => Some(PathBuf::from(&v[1..])),
        Some("") => args.get(i + 1).map(PathBuf::from),
        _ => None,
    })
}

/// The data folder: `--user-data-dir`, else %APPDATA%\launchpad as in 1.2.
pub fn data_dir(user_data_dir: Option<&Path>) -> PathBuf {
    match user_data_dir {
        Some(d) => std::path::absolute(d).unwrap_or_else(|_| d.to_path_buf()),
        None => PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("launchpad"),
    }
}

/// The lock's name, also the MsgWindow's title: "Launchpad-" and a stable
/// hash (FNV-1a, so every build agrees) of the data folder, case-folded.
pub fn key(data_dir: &Path) -> String {
    let norm = data_dir.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let hash = norm.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("Launchpad-{hash:016x}")
}

/// Held for the life of the process; the system releases it if we die.
pub struct Lock(HANDLE);

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The lock, or None if another instance holds it.
pub fn acquire(key: &str) -> Result<Option<Lock>> {
    // Local\: this session's namespace; the key already separates users.
    let name = wide(format!("Local\\{key}"));
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }.ctx("CreateMutex")?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Ok(None);
    }
    Ok(Some(Lock(handle)))
}

/// From a second instance: asks the first to show its window. False if its
/// window wasn't found (it may still be starting; this waits up to ~2 s).
pub fn show_first(key: &str) -> bool {
    let (class, title) = (wide(CLASS), wide(key));
    for _ in 0..10 {
        if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) } {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                // We were just launched by the user, so we may hand the
                // foreground on; otherwise the first's window only flashes.
                let _ = AllowSetForegroundWindow(pid);
                return PostMessageW(Some(hwnd), WM_SHOW_REQUEST, WPARAM(0), LPARAM(0)).is_ok();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

pub fn probe(args: &[String]) -> Result<Value> {
    let dir = data_dir(user_data_dir(args).as_deref());
    let key = key(&dir);
    match args.first().map(String::as_str) {
        Some("check") => {
            let lock = acquire(&key)?;
            Ok(json!({ "dataDir": dir, "key": key, "anotherInstance": lock.is_none() }))
        }
        // Holds the lock with a window, as the app will, until Ctrl+C or N s.
        Some("hold") => {
            let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0);
            let Some(_lock) = acquire(&key)? else {
                return Ok(json!({ "key": key, "held": false, "askedFirstToShow": show_first(&key) }));
            };
            let window = super::tray::MsgWindow::new(&key, |e| eprintln!("{e:?}"))?;
            eprintln!("Holding {key} for {secs} s.");
            super::tray::pump_for(secs);
            drop(window);
            Ok(json!({ "key": key, "held": true }))
        }
        _ => Err(Error::msg("Usage: instance check | hold [seconds]   [--user-data-dir <dir>]")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_flag_forms() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(user_data_dir(&a(&["check", "--user-data-dir=C:\\t d"])), Some(PathBuf::from("C:\\t d")));
        assert_eq!(user_data_dir(&a(&["--user-data-dir", "x"])), Some(PathBuf::from("x")));
        assert_eq!(user_data_dir(&a(&["--user-data-dirt"])), None);
    }

    #[test]
    fn key_ignores_case_and_slashes() {
        assert_eq!(key(Path::new("C:\\Users\\A\\AppData\\Roaming\\launchpad")), key(Path::new("c:/users/a/appdata/roaming/Launchpad/")));
        assert_ne!(key(Path::new("C:\\a")), key(Path::new("C:\\b")));
    }
}
