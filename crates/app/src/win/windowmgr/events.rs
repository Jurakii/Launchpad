//! Foreground and drag events from WinEvent hooks, on a thread of their own
//! with its own message loop (out-of-context hooks are called on the thread
//! that set them, from inside its GetMessage).
use super::{exe_path, frame, is_shell, is_snappable, is_window, own_pid, pid_of, title_of, val, visible, zoomed};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Another app's window.
    App,
    /// Launchpad's own.
    Own,
    /// The desktop, taskbar, Start, search.
    Shell,
    /// No foreground window.
    None,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::Own => "own",
            Kind::Shell => "shell",
            Kind::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Foreground {
    pub hwnd: i64,
    pub kind: Kind,
    /// False for the hidden windows some background apps bring to the front.
    pub visible: bool,
    /// Only for `App` and `Own`.
    pub title: String,
    /// Only for `App` and `Shell`.
    pub exe: Option<String>,
    pub snappable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WmEvent {
    /// The foreground window changed, or its title did. Also sent once at start.
    Foreground(Foreground),
    /// The user is moving (not resizing) a window worth snapping. Once per
    /// drag, never for Launchpad's own windows.
    DragStart { hwnd: i64 },
    /// A window's move/size loop ended. `cancelled`: Esc, seen as the mouse
    /// button still held.
    MoveEnd { hwnd: i64, cancelled: bool },
    /// Some other app's window was shown, hidden or closed (the taskbar's
    /// open-windows row). Bursts arrive as one event, a moment after.
    WindowsChanged,
}

impl WmEvent {
    /// The old helper's event line.
    pub fn to_json(&self) -> Value {
        match self {
            WmEvent::Foreground(f) => json!({ "ev": "foreground", "hwnd": f.hwnd, "kind": f.kind.as_str(), "visible": f.visible,
                                              "title": f.title, "exe": f.exe, "snappable": f.snappable }),
            WmEvent::DragStart { hwnd } => json!({ "ev": "dragStart", "hwnd": hwnd }),
            WmEvent::MoveEnd { hwnd, cancelled } => json!({ "ev": "moveEnd", "hwnd": hwnd, "cancelled": cancelled }),
            WmEvent::WindowsChanged => json!({ "ev": "windowsChanged" }),
        }
    }
}

/// The foreground window right now, as the foreground event describes it.
pub fn foreground() -> Foreground {
    let h = unsafe { GetForegroundWindow() };
    describe(h, if h.is_invalid() { 0 } else { pid_of(h) })
}

fn describe(h: HWND, pid: u32) -> Foreground {
    let (kind, exe) = if h.is_invalid() || !is_window(h) {
        (Kind::None, None)
    } else if pid == own_pid() {
        (Kind::Own, None)
    } else {
        let exe = exe_path(pid);
        (if is_shell(h, exe.as_deref()) { Kind::Shell } else { Kind::App }, exe)
    };
    Foreground {
        hwnd: val(h),
        kind,
        visible: kind != Kind::None && visible(h),
        title: if matches!(kind, Kind::App | Kind::Own) { title_of(h) } else { String::new() },
        exe,
        snappable: kind == Kind::App && is_snappable(h, pid),
    }
}

/// The hook thread; stops it and removes its hooks on drop.
pub struct Events {
    thread: u32,
    join: Option<JoinHandle<()>>,
}

impl Drop for Events {
    fn drop(&mut self) {
        let _ = unsafe { PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Starts the hook thread. `cb` is called on that thread, so it should only
/// hand the event on (to the UI thread) and return.
pub fn start(cb: impl Fn(WmEvent) + Send + 'static) -> Result<Events> {
    let (tx, rx) = mpsc::channel();
    let join = std::thread::Builder::new()
        .name("windowmgr-events".into())
        .spawn(move || run(Rc::new(cb), tx))
        .map_err(|e| Error::msg(format!("Couldn't start the window events thread: {e}")))?;
    match rx.recv() {
        Ok(Ok(thread)) => Ok(Events { thread, join: Some(join) }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => Err(Error::msg("The window events thread stopped.")),
    }
}

struct State {
    cb: Rc<dyn Fn(WmEvent)>,
    me: u32,
    // The window whose move/size loop is running, while it's still unclear
    // whether the user is moving or resizing it, and its frame when last
    // looked at.
    pending: HWND,
    pending_frame: RECT,
    location_hook: HWINEVENTHOOK,
    // The foreground window, and a hook for title changes scoped to its process.
    fg: HWND,
    fg_pid: u32,
    name_hook: HWINEVENTHOOK,
    // Pending WindowsChanged: the coalescing timer's id, 0 when none.
    changed_timer: usize,
}

/// How long after the first show/hide/destroy WindowsChanged is sent: a
/// window opening fires several, and menus or tooltips fire them in bursts.
const CHANGED_DELAY_MS: u32 = 150;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn hook(min: u32, max: u32, pid: u32, flags: u32) -> HWINEVENTHOOK {
    unsafe { SetWinEventHook(min, max, None, Some(on_event), pid, 0, flags | WINEVENT_OUTOFCONTEXT) }
}

fn unhook(h: &mut HWINEVENTHOOK) {
    if !h.is_invalid() {
        let _ = unsafe { UnhookWinEvent(*h) };
    }
    *h = HWINEVENTHOOK::default();
}

fn run(cb: Rc<dyn Fn(WmEvent)>, tx: mpsc::Sender<Result<u32>>) {
    unsafe {
        // Physical pixels on this thread too, whatever the process set.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Creates the thread's message queue, so Drop's WM_QUIT can't be lost.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    // Not WINEVENT_SKIPOWNPROCESS for the foreground: the old helper was a
    // separate process, and Launchpad's own windows must still be reported
    // (kind "own"). Its own move/size loops are skipped below, by pid.
    let mut fixed = [
        hook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, 0, 0),
        hook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, 0, WINEVENT_SKIPOWNPROCESS),
        hook(EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, 0, WINEVENT_SKIPOWNPROCESS),
    ];
    if fixed.iter().any(|h| h.is_invalid()) {
        fixed.iter_mut().for_each(unhook);
        let _ = tx.send(Err(Error::msg("SetWinEventHook failed.")));
        return;
    }
    STATE.with_borrow_mut(|s| {
        *s = Some(State {
            cb,
            me: own_pid(),
            pending: HWND::default(),
            pending_frame: RECT::default(),
            location_hook: HWINEVENTHOOK::default(),
            fg: HWND::default(),
            fg_pid: 0,
            name_hook: HWINEVENTHOOK::default(),
            changed_timer: 0,
        })
    });
    let _ = tx.send(Ok(unsafe { GetCurrentThreadId() }));
    // Whatever is in front already, so Launchpad doesn't have to wait for
    // the user to switch windows to find out.
    unsafe { on_event(HWINEVENTHOOK::default(), EVENT_SYSTEM_FOREGROUND, GetForegroundWindow(), 0, 0, 0, 0) };
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    fixed.iter_mut().for_each(unhook);
    if let Some(mut s) = STATE.take() {
        unhook(&mut s.location_hook);
        unhook(&mut s.name_hook);
    }
}

// A move/size loop starts for title-bar drags, border resizes and the
// keyboard Move/Size commands alike, so the start event alone doesn't say
// which. Asking the window what's under the cursor isn't reliable: these
// events arrive a moment late, and a quick hand can already have carried
// the cursor from a border onto the title bar or vice versa. What the
// window does is reliable: while it's being moved its size stays the
// same and its position changes; while it's being resized, every change
// is a size change. So the window is watched until it moves without
// resizing. Ignoring size changes also covers a snapped window that
// restores its normal size as the drag starts. A maximized window can't
// be resized, so dragging one is a move straight away.
unsafe extern "system" fn on_event(_: HWINEVENTHOOK, ev: u32, h: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != 0 || id_child != 0 || h.is_invalid() {
        return; // the window itself, not a part of it
    }
    // The state is only borrowed while deciding; describing the window and
    // the callback run after, so nothing that waits on messages (and could
    // deliver another event here) happens inside the borrow.
    let Some((cb, out)) = STATE.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        let s = guard.as_mut()?;
        Some((s.cb.clone(), decide(s, ev, h)))
    }) else {
        return;
    };
    let event = match out {
        Out::Nothing => return,
        Out::Foreground(h, pid) => WmEvent::Foreground(describe(h, pid)),
        Out::Event(e) => e,
    };
    cb(event);
}

unsafe extern "system" fn on_changed_timer(_: HWND, _: u32, id: usize, _: u32) {
    unsafe {
        let _ = KillTimer(None, id);
    }
    let cb = STATE.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        let s = guard.as_mut()?;
        s.changed_timer = 0;
        Some(s.cb.clone())
    });
    if let Some(cb) = cb {
        cb(WmEvent::WindowsChanged);
    }
}

enum Out {
    Nothing,
    Foreground(HWND, u32),
    Event(WmEvent),
}

fn decide(s: &mut State, ev: u32, h: HWND) -> Out {
    match ev {
        EVENT_SYSTEM_FOREGROUND => {
            unhook(&mut s.name_hook);
            s.fg = h;
            s.fg_pid = pid_of(h);
            // Switching tabs or documents changes the title but not the
            // window, so title changes are watched too, in that process only.
            if s.fg_pid != 0 {
                s.name_hook = hook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE, s.fg_pid, 0);
            }
            Out::Foreground(h, s.fg_pid)
        }
        EVENT_OBJECT_NAMECHANGE if h == s.fg => Out::Foreground(h, s.fg_pid),
        EVENT_OBJECT_NAMECHANGE => Out::Nothing,
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_SHOW | EVENT_OBJECT_HIDE => {
            // Only top-level windows (a destroyed one can't be asked; it costs one extra event).
            let top = ev == EVENT_OBJECT_DESTROY || unsafe { GetAncestor(h, GA_PARENT) } == unsafe { GetDesktopWindow() };
            if top && s.changed_timer == 0 {
                s.changed_timer = unsafe { SetTimer(None, 0, CHANGED_DELAY_MS, Some(on_changed_timer)) };
            }
            Out::Nothing
        }
        EVENT_OBJECT_LOCATIONCHANGE => {
            if h != s.pending {
                return Out::Nothing; // other windows of the same app
            }
            let now = frame(h);
            let p = s.pending_frame;
            let same_size = now.right - now.left == p.right - p.left && now.bottom - now.top == p.bottom - p.top;
            let same_place = now.left == p.left && now.top == p.top;
            // Events queue up and are handled after the fact, so several
            // can read the same current frame: one that matches the last
            // reading says nothing either way.
            if same_size && same_place {
                return Out::Nothing;
            }
            if same_size {
                stop_location_watch(s);
                return Out::Event(WmEvent::DragStart { hwnd: val(h) });
            }
            s.pending_frame = now;
            Out::Nothing
        }
        EVENT_SYSTEM_MOVESIZESTART | EVENT_SYSTEM_MOVESIZEEND => {
            let pid = pid_of(h);
            if pid == s.me {
                return Out::Nothing;
            }
            stop_location_watch(s);
            if ev == EVENT_SYSTEM_MOVESIZEEND {
                // Esc cancels a move while the button is still held; a normal
                // drop ends the loop because the button was released.
                let button = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
                let cancelled = unsafe { GetAsyncKeyState(button.0 as i32) } as u16 & 0x8000 != 0;
                return Out::Event(WmEvent::MoveEnd { hwnd: val(h), cancelled });
            }
            if !is_snappable(h, pid) {
                return Out::Nothing;
            }
            if zoomed(h) {
                return Out::Event(WmEvent::DragStart { hwnd: val(h) });
            }
            s.pending = h;
            s.pending_frame = frame(h);
            // Scoped to that one process, and only until the loop ends.
            s.location_hook = hook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, pid, 0);
            Out::Nothing
        }
        _ => Out::Nothing,
    }
}

fn stop_location_watch(s: &mut State) {
    unhook(&mut s.location_hook);
    s.pending = HWND::default();
}
