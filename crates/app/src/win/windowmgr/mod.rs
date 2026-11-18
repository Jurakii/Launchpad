//! The window organizer's Win32 side, ported from `lib/windowmgr.ps1` (and
//! the taskbar list of `lib/windows.ps1`): listing, focusing and placing
//! other apps' windows, the fullscreen walk, and (in `events`) foreground and
//! drag events. In-process now, so the old helper's JSON-lines protocol, its
//! timeout and restart logic are gone; failures are `Fail` values with the
//! helper's codes.
//!
//! Handles cross the API as `i64` (as the helper's JSON had them, and as
//! `core::organizer::OpenWindow` keeps them). Rects are physical pixels.
pub mod events;

pub use events::{foreground, start, WmEvent};

use super::display::ltrb;
use super::{arg, from_wide};
use launchpad_core::organizer::fullscreen::{decide, DecidingWindow, MonitorEntry, Reply};
use launchpad_core::organizer::picker::picker_entries;
use launchpad_core::organizer::{OpenWindow, Rect};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The helper's failure codes: `gone` (window closed), `denied` (an
/// elevated window Windows won't let us move), `failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    Gone,
    Denied,
    Failed,
}

impl Fail {
    pub fn code(self) -> &'static str {
        match self {
            Fail::Gone => "gone",
            Fail::Denied => "denied",
            Fail::Failed => "failed",
        }
    }
}

/// An open top-level window, top of the z-order first in every list.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub hwnd: i64,
    pub pid: u32,
    /// None when the process couldn't be opened (only possible in
    /// `taskbar_windows`; `windows` leaves those out).
    pub exe: Option<String>,
    pub title: String,
    /// The visible frame.
    pub frame: RECT,
    pub minimized: bool,
    pub maximized: bool,
}

impl Window {
    pub fn open_window(&self) -> OpenWindow {
        OpenWindow { hwnd: self.hwnd, exe: self.exe.clone().unwrap_or_default(), title: self.title.clone(), minimized: self.minimized }
    }

    pub fn to_json(&self) -> Value {
        json!({ "hwnd": self.hwnd, "pid": self.pid, "exe": self.exe, "title": self.title, "frame": ltrb(&self.frame),
                "minimized": self.minimized, "maximized": self.maximized })
    }
}

pub fn hwnd(v: i64) -> HWND {
    HWND(v as isize as _)
}

fn val(h: HWND) -> i64 {
    h.0 as isize as i64
}

// Windows that belong to the shell rather than to an app: the desktop,
// the taskbars, and the Start, search and Action Center hosts.
const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];
const SHELL_PROCESSES: [&str; 4] = ["StartMenuExperienceHost", "SearchHost", "SearchApp", "ShellExperienceHost"];

fn own_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn is_window(h: HWND) -> bool {
    unsafe { IsWindow(Some(h)) }.as_bool()
}

fn visible(h: HWND) -> bool {
    unsafe { IsWindowVisible(h) }.as_bool()
}

fn iconic(h: HWND) -> bool {
    unsafe { IsIconic(h) }.as_bool()
}

fn zoomed(h: HWND) -> bool {
    unsafe { IsZoomed(h) }.as_bool()
}

fn ex_style(h: HWND) -> WINDOW_EX_STYLE {
    WINDOW_EX_STYLE(unsafe { GetWindowLongW(h, GWL_EXSTYLE) } as u32)
}

fn window_rect(h: HWND) -> RECT {
    let mut r = RECT::default();
    let _ = unsafe { GetWindowRect(h, &mut r) };
    r
}

// The visible frame. GetWindowRect also counts the ~7px invisible resize
// border Windows 10 draws on the left, right and bottom of most windows;
// the DWM frame bounds don't, and that's what should line up with a zone.
fn frame(h: HWND) -> RECT {
    let mut r = RECT::default();
    if unsafe { DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, size_of::<RECT>() as u32) }.is_err() {
        r = window_rect(h);
    }
    r
}

// UWP apps keep a DWM-cloaked ApplicationFrameHost window alive in the
// background that still has a handle and a title; the taskbar doesn't show it.
fn cloaked(h: HWND) -> bool {
    let mut c = 0u32;
    let _ = unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut c as *mut _ as *mut _, 4) };
    c != 0
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    from_wide(&buf[..n.max(0) as usize])
}

// Can't hang on a busy app: for another process's window this reads the
// stored text instead of sending the window WM_GETTEXT. (For one of our own
// windows on another thread it does send it, to that thread.)
fn title_of(h: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(h) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    from_wide(&buf[..n.max(0) as usize])
}

/// Closes a handle on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// PROCESS_QUERY_LIMITED_INFORMATION is allowed even for elevated apps.
fn open_limited(pid: u32) -> Option<Owned> {
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok().map(Owned)
}

fn exe_path(pid: u32) -> Option<String> {
    let p = open_limited(pid)?;
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(p.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size) }.ok()?;
    Some(from_wide(&buf[..size as usize]))
}

fn is_shell(h: HWND, exe: Option<&str>) -> bool {
    if SHELL_CLASSES.contains(&class_of(h).as_str()) {
        return true;
    }
    let Some(stem) = exe.and_then(|e| Path::new(e).file_stem()).and_then(|s| s.to_str()) else {
        return false;
    };
    SHELL_PROCESSES.iter().any(|s| s.eq_ignore_ascii_case(stem))
}

/// Some(true) elevated, Some(false) not, None couldn't tell.
fn elevation(pid: u32) -> Option<bool> {
    let p = open_limited(pid)?;
    let mut t = HANDLE::default();
    unsafe { OpenProcessToken(p.0, TOKEN_QUERY, &mut t) }.ok()?;
    let t = Owned(t);
    let mut e = TOKEN_ELEVATION::default();
    let mut len = 0;
    unsafe { GetTokenInformation(t.0, TokenElevation, Some(&mut e as *mut _ as *mut _), size_of::<TOKEN_ELEVATION>() as u32, &mut len) }.ok()?;
    Some(e.TokenIsElevated != 0)
}

// UIPI stops a normal process from moving an elevated window.
fn blocked_by_elevation(pid: u32) -> bool {
    static SELF_ELEVATED: OnceLock<bool> = OnceLock::new();
    elevation(pid) == Some(true) && !*SELF_ELEVATED.get_or_init(|| elevation(own_pid()) == Some(true))
}

// The windows the taskbar would show: visible, top-level (no owner), not a
// tool window, and not DWM-cloaked. The naive "has a main window handle"
// approach picks up windows that are never on the real taskbar.
fn taskbar_worthy(h: HWND) -> bool {
    visible(h)
        && unsafe { GetWindow(h, GW_OWNER) }.is_err()
        && !ex_style(h).contains(WS_EX_TOOLWINDOW)
        && !cloaked(h)
}

// Worth offering zones for: the windows the taskbar would show (same
// rules as taskbar_windows), resizable ones only - a fixed-size dialog
// stretched into a zone just looks broken - and not elevated ones we
// couldn't move anyway.
fn is_snappable(h: HWND, pid: u32) -> bool {
    let style = WINDOW_STYLE(unsafe { GetWindowLongW(h, GWL_STYLE) } as u32);
    taskbar_worthy(h) && style.contains(WS_THICKFRAME) && !blocked_by_elevation(pid)
}

/// Top-level windows, top of the z-order first.
fn enum_windows() -> Vec<HWND> {
    unsafe extern "system" fn on_window(h: HWND, lp: LPARAM) -> BOOL {
        // lp is the &mut Vec passed below, alive for the whole enumeration.
        unsafe { &mut *(lp.0 as *mut Vec<HWND>) }.push(h);
        true.into()
    }
    let mut list = Vec::new();
    let _ = unsafe { EnumWindows(Some(on_window), LPARAM(&mut list as *mut _ as isize)) };
    list
}

fn window(h: HWND, pid: u32, exe: Option<String>, title: String) -> Window {
    Window { hwnd: val(h), pid, exe, title, frame: frame(h), minimized: iconic(h), maximized: zoomed(h) }
}

/// The windows a scene or the picker can hold (the helper's `windows`): the
/// ones dragStart is sent for, not Launchpad's own or the shell's, with a
/// title to tell them apart by. Stricter than `taskbar_windows`: it also
/// leaves out windows that can't be resized and elevated ones.
pub fn windows() -> Vec<Window> {
    let me = own_pid();
    enum_windows()
        .into_iter()
        .filter_map(|h| {
            let pid = pid_of(h);
            if pid == me || !visible(h) {
                return None;
            }
            let exe = exe_path(pid)?;
            if is_shell(h, Some(&exe)) || !is_snappable(h, pid) {
                return None;
            }
            let title = title_of(h);
            (!title.is_empty()).then(|| window(h, pid, Some(exe), title))
        })
        .collect()
}

/// Every window the Windows taskbar would show, for Launchpad's own taskbar
/// (`windows.ps1 -Action list`), except Launchpad's own.
pub fn taskbar_windows() -> Vec<Window> {
    let me = own_pid();
    enum_windows()
        .into_iter()
        .filter_map(|h| {
            if !taskbar_worthy(h) {
                return None;
            }
            let title = title_of(h);
            let pid = pid_of(h);
            if title.trim().is_empty() || pid == me {
                return None;
            }
            Some(window(h, pid, exe_path(pid), title))
        })
        .collect()
}

fn live(v: i64) -> std::result::Result<HWND, Fail> {
    let h = hwnd(v);
    if is_window(h) {
        Ok(h)
    } else {
        Err(Fail::Gone)
    }
}

/// Restores it if minimized and brings it to the front. Ok(false) when
/// Windows refused the foreground change (only the foreground process, or
/// one it started, may change it), as the helper replied `ok: false`.
pub fn focus(v: i64) -> std::result::Result<bool, Fail> {
    let h = live(v)?;
    unsafe {
        if iconic(h) {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        Ok(SetForegroundWindow(h).as_bool())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowMonitor {
    pub bounds: RECT,
    pub work: RECT,
    /// The window's visible frame.
    pub frame: RECT,
    pub minimized: bool,
}

/// The monitor the window is on, and where it is on it. For a minimized
/// window this is the monitor it will restore onto.
pub fn monitor(v: i64) -> std::result::Result<WindowMonitor, Fail> {
    let h = live(v)?;
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi) }.as_bool() {
        return Err(Fail::Failed);
    }
    Ok(WindowMonitor { bounds: mi.rcMonitor, work: mi.rcWork, frame: frame(h), minimized: iconic(h) })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub frame: RECT,
    /// False when the app enforces a minimum size bigger than the rect.
    pub exact: bool,
}

/// Moves the window so its visible frame fills `rect`, then maximizes it
/// there (on that rect's monitor) if `max`.
pub fn place(v: i64, rect: RECT, max: bool) -> std::result::Result<Placed, Fail> {
    let h = live(v)?;
    // Saying so up front gives a clear message instead of a window that
    // just doesn't move.
    if blocked_by_elevation(pid_of(h)) {
        return Err(Fail::Denied);
    }
    unsafe {
        // Already maximized on the right monitor (a scene putting back a
        // window that never moved): restoring and maximizing it again would
        // only make it flicker.
        if max && zoomed(h) && MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) == MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) {
            return Ok(Placed { frame: frame(h), exact: true });
        }
        // Aero Snap may already have maximized it (it does so before the
        // drag-end event arrives), and a maximized window ignores SetWindowPos.
        if zoomed(h) || iconic(h) {
            let _ = ShowWindow(h, SW_RESTORE);
        }
    }
    // Two passes at most: moving to a monitor with a different DPI makes
    // the app rescale itself and changes its invisible border, so the
    // first pass can land slightly off.
    let mut off = i32::MAX;
    for _ in 0..2 {
        if off <= 1 {
            break;
        }
        let (wr, fr) = (window_rect(h), frame(h));
        let (dl, dt, dr, db) = (fr.left - wr.left, fr.top - wr.top, wr.right - fr.right, wr.bottom - fr.bottom);
        let (w, ht) = (rect.right - rect.left, rect.bottom - rect.top);
        let flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        if let Err(e) = unsafe { SetWindowPos(h, None, rect.left - dl, rect.top - dt, w + dl + dr, ht + dt + db, flags) } {
            return Err(if e.code() == ERROR_ACCESS_DENIED.to_hresult() { Fail::Denied } else { Fail::Failed });
        }
        off = max_offset(&frame(h), &rect);
    }
    // A window maximizes on the monitor it's on, so it's moved there first.
    if max {
        let _ = unsafe { ShowWindow(h, SW_MAXIMIZE) };
    }
    Ok(Placed { frame: frame(h), exact: max || off <= 1 })
}

fn max_offset(a: &RECT, b: &RECT) -> i32 {
    [a.left - b.left, a.top - b.top, a.right - b.right, a.bottom - b.bottom].iter().map(|d| d.abs()).max().unwrap_or(0)
}

/// The panels are made topmost once; this puts that back if something took
/// it away. A window that still is topmost is left where it is, so the panel
/// doesn't jump above a menu or overlay opened later. Returns whether it
/// still was.
pub fn ensure_topmost(v: i64) -> std::result::Result<bool, Fail> {
    let h = live(v)?;
    let was = ex_style(h).contains(WS_EX_TOPMOST);
    if !was {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        let _ = unsafe { SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
    }
    Ok(was)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decider {
    pub hwnd: i64,
    pub state: DecidingWindow,
    /// One line for the panel's diagnostic log: what decided the monitor.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullscreenMonitor {
    pub monitor: RECT,
    pub window: Option<Decider>,
}

// For each monitor, the window that decides whether a fullscreen app has
// taken it over: the first one on it, going down the z-order, that covers
// all of it or isn't always-on-top. Passed over on the way: the shell's
// own windows, windows that never take focus (Launchpad's panels and
// overlays, notifications), click-through and tool windows, and smaller
// always-on-top ones, so a sticky note can't hide a fullscreen game from
// this. An own window (Launchpad's main window, whose top bar is the
// panel) ends the walk like any other, so a fullscreen app behind it
// doesn't count. The window is reported with what core's fullscreen::decide
// needs: whether it covers the monitor, is maximized, is own, and belongs to
// the foreground app (a launcher or overlay a game left behind can cover a
// monitor without being in front).
pub fn fullscreen(own: &[i64]) -> Vec<FullscreenMonitor> {
    let skip = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
    let windows: Vec<HWND> = enum_windows()
        .into_iter()
        .filter(|&h| {
            visible(h) && !iconic(h) && (ex_style(h).0 & skip.0) == 0 && !cloaked(h) && !SHELL_CLASSES.contains(&class_of(h).as_str())
        })
        .collect();
    let fg = unsafe { GetForegroundWindow() };
    let fg_pid = if fg.is_invalid() { 0 } else { pid_of(fg) };

    super::display::monitors()
        .into_iter()
        .map(|m| {
            let m = m.bounds;
            let window = windows.iter().find_map(|&h| {
                let r = window_rect(h);
                if r.left >= m.right || r.right <= m.left || r.top >= m.bottom || r.bottom <= m.top {
                    return None; // not on this monitor
                }
                let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
                let topmost = ex_style(h).contains(WS_EX_TOPMOST);
                if !covers && topmost {
                    return None;
                }
                let state = DecidingWindow {
                    covers,
                    zoomed: zoomed(h),
                    own: own.contains(&val(h)),
                    foreground: h == fg || (fg_pid != 0 && pid_of(h) == fg_pid),
                };
                let exe = exe_path(pid_of(h));
                let exe = exe.as_deref().and_then(|e| Path::new(e).file_name()).and_then(|s| s.to_str()).unwrap_or("?");
                let detail = format!(
                    "{},{},{},{}{}{}{}{}{} hwnd={} class={} exe={} title={}",
                    m.left, m.top, m.right, m.bottom,
                    if covers { " covers" } else { " stops" },
                    if state.own { " own" } else { "" },
                    if state.zoomed { " zoomed" } else { "" },
                    if topmost { " topmost" } else { "" },
                    if state.foreground { " foreground" } else { "" },
                    val(h), class_of(h), exe, title_of(h),
                );
                Some(Decider { hwnd: val(h), state, detail })
            });
            FullscreenMonitor { monitor: m, window }
        })
        .collect()
}

/// The walk in the shape `core::organizer::fullscreen::decide` takes.
pub fn fullscreen_reply(list: &[FullscreenMonitor]) -> Reply {
    let monitors = list
        .iter()
        .map(|f| MonitorEntry {
            monitor: [f.monitor.left, f.monitor.top, f.monitor.right, f.monitor.bottom].map(f64::from),
            window: f.window.as_ref().map(|d| d.state),
        })
        .collect();
    Reply { ok: true, monitors: Some(monitors) }
}

fn fail_json(f: Fail) -> Value {
    json!({ "ok": false, "code": f.code() })
}

fn num(args: &[String], name: &str) -> Result<i64> {
    arg(args, name).and_then(|s| s.parse().ok()).ok_or_else(|| Error::msg(format!("{name} <number> is required.")))
}

pub fn probe(args: &[String]) -> Result<Value> {
    let reply = |r: std::result::Result<Value, Fail>| Ok(r.unwrap_or_else(fail_json));
    match args.first().map(String::as_str) {
        None => {
            let list = windows();
            let open: Vec<OpenWindow> = list.iter().map(Window::open_window).collect();
            Ok(json!({
            "windows": list.iter().map(Window::to_json).collect::<Vec<_>>(),
            // What the window picker would offer (core::organizer::picker).
            "picker": picker_entries(&open).iter().map(|w| w.hwnd).collect::<Vec<_>>(),
            "taskbar": taskbar_windows().iter().map(Window::to_json).collect::<Vec<_>>(),
            "foreground": WmEvent::Foreground(foreground()).to_json(),
            }))
        }
        Some("monitor") => reply(monitor(num(args, "--hwnd")?).map(|m| {
            json!({ "bounds": ltrb(&m.bounds), "work": ltrb(&m.work), "frame": ltrb(&m.frame), "minimized": m.minimized })
        })),
        Some("place") => {
            let (x, y) = (num(args, "--x")? as i32, num(args, "--y")? as i32);
            let rect = RECT { left: x, top: y, right: x + num(args, "--w")? as i32, bottom: y + num(args, "--h")? as i32 };
            let max = args.iter().any(|a| a == "--max");
            reply(place(num(args, "--hwnd")?, rect, max).map(|p| json!({ "frame": ltrb(&p.frame), "exact": p.exact })))
        }
        Some("focus") => reply(focus(num(args, "--hwnd")?).map(|ok| json!({ "ok": ok }))),
        Some("topmost") => reply(ensure_topmost(num(args, "--hwnd")?).map(|was| json!({ "was": was }))),
        Some("fullscreen") => {
            let own: Vec<i64> = arg(args, "--own").unwrap_or("").split(',').filter_map(|s| s.trim().parse().ok()).collect();
            let list = fullscreen(&own);
            let monitors: Vec<Value> = list
                .iter()
                .map(|f| {
                    let w = f.window.as_ref().map(|d| {
                        json!({ "hwnd": d.hwnd, "covers": d.state.covers, "zoomed": d.state.zoomed, "own": d.state.own, "foreground": d.state.foreground })
                    });
                    json!({ "monitor": ltrb(&f.monitor), "window": w })
                })
                .collect();
            let detail: Vec<&str> = list.iter().filter_map(|f| f.window.as_ref().map(|d| d.detail.as_str())).collect();
            // core's verdict per monitor, without Explorer's hint.
            let reply = fullscreen_reply(&list);
            let taken: Vec<bool> = list
                .iter()
                .map(|f| {
                    let m = f.monitor;
                    let r = Rect::new(m.left.into(), m.top.into(), (m.right - m.left).into(), (m.bottom - m.top).into());
                    decide(Some(&reply), r, false, false)
                })
                .collect();
            Ok(json!({ "monitors": monitors, "detail": detail, "takenOver": taken }))
        }
        Some("watch") => {
            let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
            let events = start(|e| println!("{}", e.to_json()))?;
            std::thread::sleep(std::time::Duration::from_secs(secs));
            drop(events);
            Ok(json!({ "watched": secs }))
        }
        Some(other) => Err(Error::msg(format!("Unknown action {other}: monitor, place, focus, topmost, fullscreen or watch."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_offset_is_the_worst_edge() {
        let a = RECT { left: 0, top: 0, right: 100, bottom: 100 };
        assert_eq!(max_offset(&a, &a), 0);
        assert_eq!(max_offset(&RECT { left: 1, top: -3, right: 100, bottom: 102 }, &a), 3);
    }

    #[test]
    fn failure_codes_match_the_old_helper() {
        assert_eq!([Fail::Gone, Fail::Denied, Fail::Failed].map(Fail::code), ["gone", "denied", "failed"]);
        assert_eq!(place(0, RECT::default(), false), Err(Fail::Gone));
    }

    #[test]
    fn shell_processes_match_by_file_stem_ignoring_case() {
        let desktop = HWND::default(); // class "" for a null window
        assert!(is_shell(desktop, Some(r"C:\Windows\SystemApps\x\SearchHost.exe")));
        assert!(is_shell(desktop, Some(r"C:\Windows\x\startmenuexperiencehost.EXE")));
        assert!(!is_shell(desktop, Some(r"C:\Windows\notepad.exe")));
        assert!(!is_shell(desktop, None));
    }
}
