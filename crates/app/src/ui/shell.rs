//! What the organizer's windows (panels, overlays, the grid placer, the
//! picker and scenes menu) need from Win32 that Slint doesn't do: their
//! extended styles, exact physical placement, and seeing window messages
//! winit drops (AppBar notifications).
//!
//! Phase 0: the window must be created inactive and without a taskbar
//! button (`ui::run`'s winit attributes hook does that for every window made
//! through `make`); the rest is set on the HWND after the
//! first show. winit sets WS_EX_APPWINDOW, which is cleared here.
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Runs `f` once `c`'s window has its HWND. winit makes it on a later turn
/// of the event loop than `show()`, so nothing can be set on it straight away.
pub fn when_ready<C: slint::ComponentHandle + 'static>(c: &C, f: impl FnOnce(HWND) + 'static) {
    fn attempt<C: slint::ComponentHandle + 'static>(weak: slint::Weak<C>, f: Box<dyn FnOnce(HWND)>, left: u32) {
        let Some(c) = weak.upgrade() else { return };
        if let Some(h) = super::hwnd_of(c.window()) {
            return f(h);
        }
        if left > 0 {
            slint::Timer::single_shot(std::time::Duration::from_millis(5), move || attempt(weak, f, left - 1));
        }
    }
    attempt(c.as_weak(), Box::new(f), 400);
}

thread_local! {
    static MAKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Makes one of the organizer's windows (`PanelWindow::new`, …). Slint asks
/// `ui::run`'s attributes hook for the winit attributes inside `new()`,
/// before the title is known, so this flag is how the hook tells them apart.
pub fn make<T, E>(new: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    MAKING.set(true);
    let r = new();
    MAKING.set(false);
    r
}

/// For the attributes hook: an organizer window is being made.
pub fn making() -> bool {
    MAKING.get()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Never takes the foreground (the panel): clicking it leaves the user's
    /// window focused, which is the one it acts on.
    NoActivate,
    /// Mouse goes straight through (the drag overlay).
    ClickThrough,
    /// Takes the keyboard while open (grid placer, picker, scenes menu).
    Focusable,
}

/// Applies the styles of `kind`; call after each show (winit may rewrite them).
pub fn style(h: HWND, kind: Kind) {
    let mut add = WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
    match kind {
        Kind::NoActivate => add |= WS_EX_NOACTIVATE.0,
        Kind::ClickThrough => add |= WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0,
        Kind::Focusable => {}
    }
    unsafe {
        let ex = (GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 | add) & !WS_EX_APPWINDOW.0;
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex as isize);
        if kind == Kind::ClickThrough {
            // A layered window shows nothing until it's given its attributes.
            let _ = SetLayeredWindowAttributes(h, Default::default(), 255, LWA_ALPHA);
        }
    }
}

/// Moves and sizes the window to `r` (physical pixels), topmost, without activating it.
pub fn place(h: HWND, r: RECT) {
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// Clips what's drawn and clickable to the window's top `height` pixels.
/// Windows keeps a frameless window at least ~39 px tall (as in 1.2), so the
/// 32 px panel would otherwise cover the top of maximized windows below it.
pub fn clip_height(h: HWND, width: i32, height: i32) {
    unsafe {
        let rgn = windows::Win32::Graphics::Gdi::CreateRectRgn(0, 0, width, height);
        // The system owns the region from here on.
        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(h, Some(rgn), true);
    }
}

type Handler = Box<dyn Fn(HWND, u32, WPARAM, LPARAM) -> Option<LRESULT>>;

thread_local! {
    // Kept here rather than in dwRefData so a handler can be swapped safely.
    static HANDLERS: RefCell<Vec<(isize, Handler)>> = const { RefCell::new(Vec::new()) };
}

const SUBCLASS_ID: usize = 0x4C50_4F52; // "LPOR"

/// Sees the window's messages before winit does. `f` returns Some to
/// handle one (winit then never sees it). It runs inside the window
/// procedure: it may post or set state, but must not destroy this window.
/// Removed when the window is destroyed. One handler per window.
pub fn subclass(h: HWND, f: impl Fn(HWND, u32, WPARAM, LPARAM) -> Option<LRESULT> + 'static) {
    HANDLERS.with_borrow_mut(|list| {
        list.retain(|(k, _)| *k != h.0 as isize);
        list.push((h.0 as isize, Box::new(f)));
    });
    unsafe {
        let _ = SetWindowSubclass(h, Some(proc), SUBCLASS_ID, 0);
    }
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    if msg == WM_NCDESTROY {
        HANDLERS.with_borrow_mut(|list| list.retain(|(k, _)| *k != h.0 as isize));
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(proc), SUBCLASS_ID);
        }
    } else {
        // Taken out while it runs, so a handler that (indirectly) subclasses
        // another window doesn't find the list borrowed.
        let taken = HANDLERS.with_borrow_mut(|list| list.iter().position(|(k, _)| *k == h.0 as isize).map(|i| list.remove(i)));
        if let Some((key, f)) = taken {
            let r = f(h, msg, wp, lp);
            HANDLERS.with_borrow_mut(|list| {
                if !list.iter().any(|(k, _)| *k == key) {
                    list.push((key, f));
                }
            });
            if let Some(r) = r {
                return r;
            }
        }
    }
    unsafe { DefSubclassProc(h, msg, wp, lp) }
}
