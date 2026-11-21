//! Keeps each new window hidden from the screen (DWM cloak) until it has
//! painted, so it never shows a blank white frame first. winit registers its
//! class without a background brush, and Windows drops what Slint draws
//! before the window is mapped, so the first frame on screen was empty.
//!
//! A thread CBT hook catches every top-level window winit creates on the UI
//! thread; its subclass cloaks it as it's shown and uncloaks it one frame
//! after its first WM_PAINT (or after 300 ms, whatever happens). It also
//! gives the window the app icon, which winit's class leaves blank (taskbar,
//! Alt+Tab, Task Manager).
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

const ID: usize = 0x4C50_434B; // "LPCK"
const WINIT_CLASS: &str = "Window Class";

/// Installs the hook for the calling (UI) thread. Call once, before any window.
pub fn install() {
    unsafe {
        if let Err(e) = SetWindowsHookExW(WH_CBT, Some(cbt), None, GetCurrentThreadId()) {
            eprintln!("Window cloak hook: {e}");
        }
    }
}

unsafe extern "system" fn cbt(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HCBT_CREATEWND as i32 {
        let h = HWND(wp.0 as *mut _);
        let create = unsafe { &*(*(lp.0 as *const CBT_CREATEWNDW)).lpcs };
        let top_level = (create.style & WS_CHILD.0 as i32) == 0;
        let mut class = [0u16; 32];
        let n = unsafe { GetClassNameW(h, &mut class) } as usize;
        if top_level && String::from_utf16_lossy(&class[..n]) == WINIT_CLASS {
            unsafe {
                let _ = SetWindowSubclass(h, Some(proc), ID, 0);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

fn set_cloak(h: HWND, on: bool) {
    let v = BOOL::from(on);
    unsafe {
        let _ = DwmSetWindowAttribute(h, DWMWA_CLOAK, &v as *const _ as _, size_of::<BOOL>() as u32);
    }
}

fn set_icons(h: HWND) {
    // Loaded once and kept for the life of the process; every window shares them.
    static ICONS: std::sync::OnceLock<[isize; 2]> = std::sync::OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        [SM_CXICON, SM_CXSMICON].map(|m| super::tray::app_icon(unsafe { GetSystemMetrics(m) }).map_or(0, |i| i.0 as isize))
    });
    for (kind, icon) in [ICON_BIG, ICON_SMALL].into_iter().zip(icons) {
        if *icon != 0 {
            unsafe { SendMessageW(h, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(*icon))) };
        }
    }
}

fn done(h: HWND) {
    set_cloak(h, false);
    unsafe {
        let _ = KillTimer(Some(h), ID);
        let _ = RemoveWindowSubclass(h, Some(proc), ID);
    }
}

/// `data` (the subclass's ref data) is 1 once the window is cloaked.
unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, data: usize) -> LRESULT {
    match msg {
        WM_SHOWWINDOW if wp.0 != 0 && data == 0 => {
            set_cloak(h, true);
            set_icons(h);
            unsafe {
                let _ = SetWindowSubclass(h, Some(proc), ID, 1);
                SetTimer(Some(h), ID, 300, None);
            }
        }
        WM_PAINT if data == 1 => {
            let r = unsafe { DefSubclassProc(h, msg, wp, lp) };
            // Slint presents from inside WM_PAINT; one more frame for DWM to take it.
            unsafe { SetTimer(Some(h), ID, 20, None) };
            return r;
        }
        WM_TIMER if wp.0 == ID => {
            done(h);
            return LRESULT(0);
        }
        WM_NCDESTROY => unsafe {
            let _ = KillTimer(Some(h), ID);
            let _ = RemoveWindowSubclass(h, Some(proc), ID);
        },
        _ => {}
    }
    unsafe { DefSubclassProc(h, msg, wp, lp) }
}
