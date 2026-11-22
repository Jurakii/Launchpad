//! "Start with Windows" (Electron's `setLoginItemSettings`): a value under
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
//!
//! 1.2.x wrote `electron.app.Launchpad` = `"C:\Program Files\Launchpad\Launchpad.exe"`
//! (no arguments). 1.3 writes `Launchpad`; `migrate_from_electron` moves the
//! setting over once. Task Manager's enable/disable lives beside it in
//! Explorer\StartupApproved\Run (first byte even = enabled).
use super::{require_yes, wide};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::*;

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
pub const NAME: &str = "Launchpad";
/// What Electron named it for 1.2.x: "electron.app." + productName.
pub const ELECTRON_NAME: &str = "electron.app.Launchpad";

fn check(e: WIN32_ERROR, what: &str) -> Result<()> {
    if e == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(Error::msg(format!("{what}: {}", windows::core::Error::from(e.to_hresult()).message())))
    }
}

/// Reads a value: None if it isn't there.
fn get(key: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> Result<Option<Vec<u8>>> {
    get_in(HKEY_CURRENT_USER, key, name, flags)
}

fn get_in(root: HKEY, key: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> Result<Option<Vec<u8>>> {
    let (key, name) = (wide(key), wide(name));
    let (k, n) = (PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()));
    let mut len = 0u32;
    // Size first, then the data; a value changing in between just fails this read.
    let e = unsafe { RegGetValueW(root, k, n, flags, None, None, Some(&mut len)) };
    if e == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(e, "Reading the Run key")?;
    let mut buf = vec![0u8; len as usize];
    check(unsafe { RegGetValueW(root, k, n, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) }, "Reading the Run key")?;
    buf.truncate(len as usize);
    Ok(Some(buf))
}

/// The command a Run value holds.
pub fn read(name: &str) -> Result<Option<String>> {
    Ok(get(RUN, name, RRF_RT_REG_SZ)?.map(|b| {
        let w: Vec<u16> = b.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
        super::from_wide(&w)
    }))
}

/// Task Manager's switch for a Run value: None if it was never touched.
pub fn approved(name: &str) -> Option<bool> {
    let b = get(APPROVED, name, RRF_RT_REG_BINARY).ok()??;
    Some(b.first().is_some_and(|f| f & 1 == 0))
}

pub fn write(name: &str, command: &str) -> Result<()> {
    let (key, name, data) = (wide(RUN), wide(name), wide(command));
    check(
        unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32)
        },
        "Writing the Run key",
    )
}

/// Removes a Run value and its Task Manager switch. False if it wasn't there.
pub fn remove(name: &str) -> Result<bool> {
    let n = wide(name);
    let del = |key: &str| unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(wide(key).as_ptr()), PCWSTR(n.as_ptr())) };
    let _ = del(APPROVED);
    match del(RUN) {
        e if e == ERROR_FILE_NOT_FOUND => Ok(false),
        e => check(e, "Removing the Run value").map(|_| true),
    }
}

/// The command for this exe: its quoted path, as Electron wrote it.
pub fn command() -> Result<String> {
    let exe = std::env::current_exe().map_err(|e| Error::msg(format!("Finding this exe: {e}")))?;
    Ok(format!("\"{}\"", exe.display()))
}

/// Whether Launchpad starts with Windows (the Settings checkbox, the tray
/// menu tick). Like 1.2 it only reads the Run value, not Task Manager's switch.
pub fn is_enabled() -> bool {
    matches!((read(NAME), command()), (Ok(Some(v)), Ok(c)) if v.eq_ignore_ascii_case(&c))
}

#[allow(dead_code)] // Phase 3: Settings and the tray menu; not run here, it writes the real key
/// Turning it on also clears a Task Manager "disabled", so it really starts.
pub fn set_enabled(on: bool) -> Result<()> {
    remove(NAME)?;
    if on {
        write(NAME, &command()?)?;
    }
    Ok(())
}

#[allow(dead_code)] // Phase 3, first run
/// Once, on 1.3's first run: if 1.2.x's value is there, removes it and turns
/// ours on when it was on (present and not disabled in Task Manager).
/// Returns whether ours was turned on.
pub fn migrate_from_electron() -> Result<bool> {
    if read(ELECTRON_NAME)?.is_none() {
        return Ok(false);
    }
    let was_on = approved(ELECTRON_NAME) != Some(false);
    remove(ELECTRON_NAME)?;
    if was_on {
        set_enabled(true)?;
    }
    Ok(was_on)
}

fn describe(name: &str) -> Result<Value> {
    Ok(json!({ "name": name, "command": read(name)?, "approved": approved(name) }))
}

pub fn probe(args: &[String]) -> Result<Value> {
    match args.first().map(String::as_str) {
        None => Ok(json!({ "thisExe": command()?, "enabled": is_enabled(), "launchpad": describe(NAME)?, "electron": describe(ELECTRON_NAME)? })),
        // Writes, reads back and removes a test-only value; never touches the real ones.
        Some("selftest") => {
            require_yes(args, "autostart selftest")?;
            const TEST: &str = "LaunchpadPhase2Test";
            if read(TEST)?.is_some() {
                return Err(Error::msg(format!("{TEST} already exists; remove it by hand first.")));
            }
            let cmd = command()?;
            write(TEST, &cmd)?;
            let back = read(TEST);
            let removed = remove(TEST); // before any `?`, so the value never stays
            let back = back?;
            let gone = read(TEST)?.is_none();
            Ok(json!({ "wrote": cmd, "matches": back.as_deref() == Some(cmd.as_str()), "readBack": back, "removed": removed?, "goneAfter": gone }))
        }
        Some(a) => Err(Error::msg(format!("Unknown action {a}. Usage: autostart [selftest --yes]"))),
    }
}


/// A text value under HKEY_CURRENT_USER, if it's there.
pub fn hkcu_string(key: &str, name: &str) -> Option<String> {
    reg_string(HKEY_CURRENT_USER, key, name)
}

/// A text value under HKEY_LOCAL_MACHINE, if it's there.
pub fn hklm_string(key: &str, name: &str) -> Option<String> {
    reg_string(HKEY_LOCAL_MACHINE, key, name)
}

/// The names of the keys directly under HKEY_LOCAL_MACHINE\`key`.
pub fn hklm_subkeys(key: &str) -> Vec<String> {
    let k = wide(key);
    let mut h = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(k.as_ptr()), None, KEY_READ, &mut h) } != ERROR_SUCCESS {
        return vec![];
    }
    let mut out = vec![];
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        if unsafe { RegEnumKeyExW(h, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) } != ERROR_SUCCESS {
            break;
        }
        out.push(super::from_wide(&buf[..len as usize]));
    }
    let _ = unsafe { RegCloseKey(h) };
    out
}

fn reg_string(root: HKEY, key: &str, name: &str) -> Option<String> {
    let b = get_in(root, key, name, RRF_RT_REG_SZ).ok()??;
    let w: Vec<u16> = b.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
    Some(super::from_wide(&w).trim_end_matches('\0').to_string())
}
