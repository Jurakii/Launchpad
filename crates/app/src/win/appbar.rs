//! Docking a window (an organizer panel) as an AppBar along the top of a
//! monitor, so the strip it covers comes out of the work area and maximized
//! windows stop below it. Ported from windowmgr.ps1's `appbarSet` /
//! `appbarRemove`.
//!
//! Cleanup: Explorer does *not* reclaim the space when the bar's window
//! dies, but it accepts ABM_REMOVE for a handle whose window is gone. So a
//! clean exit calls `remove_all`, a panic or crash runs it from
//! `install_crash_cleanup`'s hooks, and a hard kill is healed on the next
//! start: the app keeps the `docked()` handles in appbars.json and passes
//! each to `remove_stale` before any panel docks. (appbar-guard.ps1 is gone.)
use super::display::{ltrb, monitors, work_area};
use super::windowmgr::{hwnd, Fail};
use super::{arg, wide};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, LPTOP_LEVEL_EXCEPTION_FILTER};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The message Explorer sends a docked window its notifications with
/// (wParam: `ABN_POSCHANGED` re-dock, `ABN_FULLSCREENAPP` with lParam 1/0).
/// Phase 0: winit drops it, so the panel window needs a subclass to see it.
/// (`ABN_*` are in `windows::Win32::UI::Shell`.)
pub const CALLBACK_MSG: u32 = WM_APP + 0x42;

/// The AppBars registered by this process, and the rect each was last given.
static DOCKED: Mutex<Vec<(i64, RECT)>> = Mutex::new(Vec::new());

fn data(h: i64) -> APPBARDATA {
    APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: hwnd(h), ..Default::default() }
}

fn msg(m: u32, d: &mut APPBARDATA) -> usize {
    unsafe { SHAppBarMessage(m, d) }
}

fn last_rect(h: i64) -> Option<RECT> {
    DOCKED.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == h).map(|(_, r)| *r)
}

fn store(h: i64, r: Option<RECT>) {
    let mut list = DOCKED.lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|(k, _)| *k != h);
    if let Some(r) = r {
        list.push((h, r));
    }
}

/// The message Explorer broadcasts when it restarts, forgetting AppBars:
/// call `set` with `renew` then.
pub fn taskbar_created() -> u32 {
    unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
}

/// Docks the window along the top of `rect` (its height is the bar's),
/// registering it first, and returns the rect as Explorer adjusted it.
/// `renew`: Explorer restarted and forgot it. The lock is never held across
/// SHAppBarMessage: Explorer sends ABN_* to every bar meanwhile, ours too,
/// and a handler that docks again must not deadlock.
pub fn set(h: i64, rect: RECT, renew: bool) -> std::result::Result<RECT, Fail> {
    if !unsafe { IsWindow(Some(hwnd(h))) }.as_bool() {
        return Err(Fail::Gone);
    }
    if renew {
        store(h, None);
    }
    let mut last = last_rect(h);
    if last.is_none() {
        let mut reg = data(h);
        reg.uCallbackMessage = CALLBACK_MSG;
        if msg(ABM_NEW, &mut reg) == 0 {
            // Still registered by a run that has since died (or before a
            // renew): start over so the callback message is set again.
            msg(ABM_REMOVE, &mut data(h));
            if msg(ABM_NEW, &mut reg) == 0 {
                return Err(Fail::Failed);
            }
        }
        last = Some(RECT::default());
        store(h, last);
    }
    let height = rect.bottom - rect.top;
    let mut d = data(h);
    d.uEdge = ABE_TOP;
    d.rc = rect;
    // Moves it below anything already docked there, like a Windows
    // taskbar at the top. Only the top edge is taken from the answer.
    msg(ABM_QUERYPOS, &mut d);
    d.rc.bottom = d.rc.top + height;
    // Setting the same rect again would still notify every other AppBar,
    // Launchpad's panels on other monitors included, and they'd answer by
    // setting theirs again.
    if last != Some(d.rc) {
        msg(ABM_SETPOS, &mut d);
        store(h, Some(d.rc));
    }
    Ok(d.rc)
}

/// Gives the AppBar's space back. Works on a window that's already gone
/// too: Explorer only needs the handle.
pub fn remove(h: i64) {
    msg(ABM_REMOVE, &mut data(h));
    store(h, None);
}

/// For a handle a previous run left docked (from appbars.json): removes it
/// only if no window has that handle now (one that does is a reused handle
/// and left alone). Returns whether it was removed.
pub fn remove_stale(h: i64) -> bool {
    if unsafe { IsWindow(Some(hwnd(h))) }.as_bool() {
        return false;
    }
    remove(h);
    true
}

/// The handles docked now, for appbars.json.
pub fn docked() -> Vec<i64> {
    DOCKED.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(k, _)| *k).collect()
}

/// Removes every AppBar this process registered (clean exit).
pub fn remove_all() {
    for h in docked() {
        remove(h);
    }
}

// From a crashing thread: never wait for the lock (the crash may have
// happened while it was held; the next start heals that case).
fn remove_all_crashing() {
    let list = match DOCKED.try_lock() {
        Ok(l) => l.clone(),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().clone(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    for (h, _) in list {
        msg(ABM_REMOVE, &mut data(h));
    }
}

static PREVIOUS_FILTER: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn on_crash(info: *const EXCEPTION_POINTERS) -> i32 {
    remove_all_crashing();
    let prev = PREVIOUS_FILTER.load(Ordering::SeqCst);
    if prev != 0 {
        // prev is the filter SetUnhandledExceptionFilter handed back.
        let f: LPTOP_LEVEL_EXCEPTION_FILTER = unsafe { std::mem::transmute(prev) };
        if let Some(f) = f {
            return unsafe { f(info) };
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

/// Once, at start: a panic hook (before the previous one runs, then it
/// does) and an unhandled-exception filter that give back every AppBar.
pub fn install_crash_cleanup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            remove_all_crashing();
            prev(info);
        }));
        let old = unsafe { SetUnhandledExceptionFilter(Some(on_crash)) };
        PREVIOUS_FILTER.store(old.map_or(0, |f| f as usize), Ordering::SeqCst);
    });
}

fn work_areas() -> Value {
    json!({ "monitors": monitors().iter().map(|m| ltrb(&m.work)).collect::<Vec<_>>(), "primary": ltrb(&work_area()) })
}

// Handles sent messages (Explorer's ABN_* and the like) for a while.
fn pump(d: Duration) {
    let end = Instant::now() + d;
    let mut m = MSG::default();
    while Instant::now() < end {
        unsafe {
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(Duration::from_millis(15)); // shortcut: a probe loop, the app has a real one
    }
}

fn dock(secs: u64) -> Result<Value> {
    install_crash_cleanup();
    let before = work_areas();
    let primary = monitors().into_iter().find(|m| m.primary).ok_or_else(|| Error::msg("No primary monitor."))?;
    let b = primary.bounds;
    let class = wide("STATIC");
    let h: HWND = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            w!("Launchpad AppBar probe"),
            WS_POPUP,
            b.left, b.top, b.right - b.left, 32,
            None, None, None, None,
        )
    }
    .map_err(|e| Error::msg(format!("CreateWindowEx: {}", e.message())))?;
    let v = h.0 as isize as i64;
    // If this run dies, `--probe appbar remove-stale --hwnd <this>` heals it.
    eprintln!("docking hwnd {v}");
    let result = set(v, RECT { left: b.left, top: b.top, right: b.right, bottom: b.top + 32 }, false);
    let out = match result {
        Ok(rc) => unsafe {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top, SWP_NOACTIVATE);
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            pump(Duration::from_millis(300));
            let during = work_areas();
            // A second set with the same rect must not SETPOS again.
            let again = set(v, RECT { left: b.left, top: b.top, right: b.right, bottom: b.top + 32 }, false);
            pump(Duration::from_secs(secs));
            json!({ "hwnd": v, "rect": ltrb(&rc), "again": again.map(|r| ltrb(&r)).unwrap_or(Value::Null), "docked": docked(), "during": during })
        },
        Err(f) => json!({ "hwnd": v, "error": f.code() }),
    };
    remove_all();
    pump(Duration::from_millis(400));
    let _ = unsafe { DestroyWindow(h) };
    let mut out = out;
    out["before"] = before;
    out["after"] = work_areas();
    Ok(out)
}

pub fn probe(args: &[String]) -> Result<Value> {
    match args.first().map(String::as_str) {
        None => Ok(json!({ "docked": docked(), "workAreas": work_areas(), "taskbarCreated": taskbar_created(), "callbackMsg": CALLBACK_MSG })),
        Some("dock") => dock(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3)),
        Some("remove-stale") => {
            let h = arg(args, "--hwnd").and_then(|s| s.parse().ok()).ok_or_else(|| Error::msg("--hwnd <number> is required."))?;
            let removed = remove_stale(h);
            pump(Duration::from_millis(400));
            Ok(json!({ "removed": removed, "workAreas": work_areas() }))
        }
        Some(other) => Err(Error::msg(format!("Unknown action {other}: dock [seconds] or remove-stale --hwnd H."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docked_list_tracks_store_and_remove() {
        store(-7, Some(RECT { left: 0, top: 0, right: 10, bottom: 32 }));
        assert!(docked().contains(&-7));
        assert_eq!(last_rect(-7).map(|r| r.bottom), Some(32));
        store(-7, None);
        assert!(!docked().contains(&-7));
    }
}
