//! Shutdown, restart, sleep (lib\power.ps1) and emptying the Recycle Bin
//! (main.js `system:emptyRecycleBin`, which ran Clear-RecycleBin).
//! None needs elevation: SeShutdownPrivilege is held (disabled) by every
//! interactive user's token and only has to be enabled.
use super::{require_yes, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, E_UNEXPECTED, HANDLE};
use windows::Win32::Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{InitiateSystemShutdownExW, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER, SHTDN_REASON_MINOR_OTHER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};

fn enable_shutdown_privilege() -> Result<()> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) }.ctx("OpenProcessToken")?;
    let mut tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, ..Default::default() };
    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
    let r = unsafe { LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut tp.Privileges[0].Luid) }
        .and_then(|_| unsafe { AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None) });
    let _ = unsafe { CloseHandle(token) };
    // A token without the privilege still "succeeds" here; the shutdown call then fails with access denied.
    r.ctx("Enabling SeShutdownPrivilege")
}

/// `shutdown.exe /s|/r /t 0`: now, no forced app close (apps may still ask to save).
fn shut_down(reboot: bool) -> Result<()> {
    enable_shutdown_privilege()?;
    let reason = SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED;
    unsafe { InitiateSystemShutdownExW(PCWSTR::null(), PCWSTR::null(), 0, false, reboot, reason) }
        .ctx(if reboot { "Restart" } else { "Shut down" })
}

pub fn shutdown() -> Result<()> {
    shut_down(false)
}

pub fn restart() -> Result<()> {
    shut_down(true)
}

/// Sleep with the .ps1's `SetSuspendState 0,1,0`: no hibernate, force, wake events on.
/// (Through rundll32 those numbers never reached the function, so on a PC
/// with hibernation on the old button may have hibernated; this one sleeps.)
pub fn sleep() -> Result<()> {
    enable_shutdown_privilege()?;
    if unsafe { SetSuspendState(false, true, false) } {
        Ok(())
    } else {
        Err(Error::msg(format!("Sleep failed: {}", windows::core::Error::from_thread().message())))
    }
}

/// All drives' Recycle Bins, no confirmation, no progress, no sound, like
/// `Clear-RecycleBin -Force`. Already empty counts as success.
pub fn empty_recycle_bin() -> Result<()> {
    match unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) } {
        Err(e) if e.code() == E_UNEXPECTED => Ok(()), // what an empty bin returns
        r => r.ctx("Emptying the Recycle Bin"),
    }
}

pub fn probe(args: &[String]) -> Result<Value> {
    let run = |what: &str, f: fn() -> Result<()>| -> Result<Value> {
        require_yes(args, what)?;
        f()?;
        Ok(json!({ "ok": true }))
    };
    match args.first().map(String::as_str) {
        None => Ok(json!({ "actions": ["shutdown", "restart", "sleep", "empty-recycle-bin"], "note": "each needs --yes" })),
        Some("shutdown") => run("Shutting down", shutdown),
        Some("restart") => run("Restarting", restart),
        Some("sleep") => run("Sleep", sleep),
        Some("empty-recycle-bin") => run("Emptying the Recycle Bin", empty_recycle_bin),
        Some(other) => Err(Error::msg(format!("No power action {other}. Try shutdown, restart, sleep, empty-recycle-bin."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_refuses_without_yes() {
        for action in ["shutdown", "restart", "sleep", "empty-recycle-bin"] {
            let e = probe(&[action.to_string()]).unwrap_err().to_string();
            assert!(e.contains("--yes"), "{action}: {e}");
        }
        assert!(probe(&["reboot".to_string(), "--yes".to_string()]).is_err());
    }
}
