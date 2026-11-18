//! The notification-area icon (Electron's `Tray` and `Menu`), and
//! `MsgWindow`: the one hidden window every window-message API here talks
//! through (tray callbacks, `WM_HOTKEY`, the single-instance "show yourself",
//! `TaskbarCreated`).
//!
//! `MsgWindow` is a hidden top-level window, not an `HWND_MESSAGE` one:
//! message-only windows get no broadcasts, and `TaskbarCreated` (Explorer
//! restarted, re-add the icon) is a broadcast. It is never shown, so it is not
//! in the taskbar or Alt+Tab. Create it on the UI thread; whatever pumps that
//! thread's messages (winit's loop in Phase 3, `pump_for` in probes) drives it.
use super::{wide, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_CLOSE_EVENT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CLASS: &str = "LaunchpadMsgWindow";
/// Posted by a second instance (`instance::show_first`).
pub const WM_SHOW_REQUEST: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1; // NINF_KEY

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A registered `hotkey::Hotkey` with this id was pressed.
    Hotkey(i32),
    /// Left click (or Enter/Space) on the tray icon.
    TrayClick,
    /// Right click (or the menu key) on the tray icon; show the menu at x, y.
    TrayMenu { x: i32, y: i32 },
    BalloonShown,
    BalloonClick,
    /// Timed out or dismissed.
    BalloonGone,
    /// Another launch asked this instance to show its window.
    ShowRequest,
    /// Explorer (re)started: call `Tray::readd`.
    TaskbarCreated,
    /// A display was added, removed or rearranged, its scaling changed, or
    /// a work area changed (an AppBar docked, the taskbar moved).
    DisplayChange,
}

type Callback = Box<dyn Fn(Event)>;

pub struct MsgWindow {
    hwnd: HWND,
}

impl MsgWindow {
    /// `name` is what `instance::show_first` looks for (`instance::key`).
    /// `on_event` runs inside the window procedure, so it may be re-entered
    /// (e.g. while `popup_menu` runs its loop); it must not drop the window.
    pub fn new(name: &str, on_event: impl Fn(Event) + 'static) -> Result<MsgWindow> {
        let class = wide(CLASS);
        let instance = unsafe { GetModuleHandleW(None) }.ctx("GetModuleHandle")?;
        static REGISTERED: OnceLock<()> = OnceLock::new();
        REGISTERED.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            // RegisterClassExW copies the name; failure shows up in CreateWindowExW.
            unsafe { RegisterClassExW(&wc) };
        });
        let title = wide(name);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .ctx("CreateWindowEx")?;
        let cb: Box<Callback> = Box::new(Box::new(on_event));
        // Freed in WM_NCDESTROY; the window only exists on this thread.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(cb) as isize) };
        Ok(MsgWindow { hwnd })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for MsgWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn taskbar_created() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(windows::core::w!("TaskbarCreated")) })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Callback;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    if msg == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
        }
        return LRESULT(0);
    }
    let event = match msg {
        WM_HOTKEY => Some(Event::Hotkey(wparam.0 as i32)),
        WM_SHOW_REQUEST => Some(Event::ShowRequest),
        // NOTIFYICON_VERSION_4: event in the low word, anchor point in wparam.
        WM_TRAY => match lparam.0 as u32 & 0xFFFF {
            NIN_SELECT | NIN_KEYSELECT => Some(Event::TrayClick),
            WM_CONTEXTMENU => Some(Event::TrayMenu { x: wparam.0 as u16 as i16 as i32, y: (wparam.0 >> 16) as u16 as i16 as i32 }),
            NIN_BALLOONSHOW => Some(Event::BalloonShown),
            NIN_BALLOONUSERCLICK => Some(Event::BalloonClick),
            NIN_BALLOONTIMEOUT | NIN_BALLOONHIDE => Some(Event::BalloonGone),
            _ => None,
        },
        m if m == taskbar_created() => Some(Event::TaskbarCreated),
        WM_DISPLAYCHANGE | WM_DPICHANGED => Some(Event::DisplayChange),
        WM_SETTINGCHANGE if wparam.0 as u32 == SPI_SETWORKAREA.0 => Some(Event::DisplayChange),
        _ => None,
    };
    match event {
        Some(e) => {
            // The box lives until WM_NCDESTROY, which can't run inside this call
            // unless the callback destroys the window (documented as not allowed).
            unsafe { (*ptr)(e) };
            LRESULT(0)
        }
        None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// The app icon. shortcut: read from build\icon.ico beside or above the exe
/// (the repo when run from target\); Phase 6 embeds it as a resource.
pub fn app_icon_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(4).map(|d| d.join("build").join("icon.ico")).find(|p| p.is_file())
}

/// The icon in the notification area. Removed on drop.
pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    tip: String,
}

impl Tray {
    pub fn new(window: &MsgWindow, icon: &Path, tip: &str) -> Result<Tray> {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
        let icon = unsafe { LoadImageW(None, PCWSTR(wide(icon).as_ptr()), IMAGE_ICON, size, size, LR_LOADFROMFILE) }
            .ctx(&format!("Loading {}", icon.display()))?;
        let tray = Tray { hwnd: window.hwnd(), icon: HICON(icon.0), tip: tip.to_string() };
        tray.add()?;
        Ok(tray)
    }

    /// Adds the icon again after `Event::TaskbarCreated`.
    pub fn readd(&self) -> Result<()> {
        self.delete();
        self.add()
    }

    pub fn set_tooltip(&mut self, tip: &str) -> Result<()> {
        self.tip = tip.to_string();
        self.notify(NIM_MODIFY, &self.data())
    }

    /// Where the icon is on screen (physical pixels), if Explorer says.
    pub fn rect(&self) -> Option<(i32, i32, i32, i32)> {
        let id = NOTIFYICONIDENTIFIER { cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32, hWnd: self.hwnd, uID: TRAY_ID, ..Default::default() };
        let r = unsafe { Shell_NotifyIconGetRect(&id) }.ok()?;
        Some((r.left, r.top, r.right, r.bottom))
    }

    /// The icon's NOTIFYICONDATA with icon, tooltip and callback set (notify.rs adds the balloon).
    pub(super) fn data(&self) -> NOTIFYICONDATAW {
        let mut d = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        copy_into(&mut d.szTip, &self.tip);
        d
    }

    pub(super) fn notify(&self, op: NOTIFY_ICON_MESSAGE, d: &NOTIFYICONDATAW) -> Result<()> {
        if unsafe { Shell_NotifyIconW(op, d) }.as_bool() {
            Ok(())
        } else {
            Err(Error::msg("Shell_NotifyIcon failed (is Explorer running?)."))
        }
    }

    fn add(&self) -> Result<()> {
        let mut d = self.data();
        self.notify(NIM_ADD, &d)?;
        d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        self.notify(NIM_SETVERSION, &d)
    }

    fn delete(&self) {
        let _ = self.notify(NIM_DELETE, &self.data());
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.delete();
        unsafe {
            let _ = DestroyIcon(self.icon);
        }
    }
}

/// NUL-terminated, truncated to fit.
pub(super) fn copy_into(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

pub enum MenuItem<'a> {
    /// (id, label); ids are nonzero.
    Item(u32, &'a str),
    Check(u32, &'a str, bool),
    Separator,
}

/// Shows a popup menu at x, y (from `Event::TrayMenu`) and blocks, running a
/// modal message loop, until it closes. The chosen id, or None.
pub fn popup_menu(hwnd: HWND, x: i32, y: i32, items: &[MenuItem]) -> Option<u32> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in items {
            let _ = match *item {
                MenuItem::Item(id, label) => AppendMenuW(menu, MF_STRING, id as usize, PCWSTR(wide(label).as_ptr())),
                MenuItem::Check(id, label, on) => {
                    AppendMenuW(menu, if on { MF_STRING | MF_CHECKED } else { MF_STRING }, id as usize, PCWSTR(wide(label).as_ptr()))
                }
                MenuItem::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
            };
        }
        // Without the foreground the menu won't close when clicking elsewhere;
        // the WM_NULL afterwards is the documented fix for it closing at once
        // the next time (KB135788).
        let _ = SetForegroundWindow(hwnd);
        let align = if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
        let id = TrackPopupMenu(menu, align | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, x, y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        (id.0 != 0).then_some(id.0 as u32)
    }
}

static PUMP_THREAD: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn on_ctrl(kind: u32) -> BOOL {
    let _ = unsafe { PostThreadMessageW(PUMP_THREAD.load(Ordering::SeqCst), WM_QUIT, WPARAM(0), LPARAM(0)) };
    if kind == CTRL_CLOSE_EVENT {
        // The process dies when this returns: give the loop time to remove the icon.
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    true.into()
}

/// For probes: runs this thread's message loop for `seconds`, or until
/// Ctrl+C / the console closes, so the caller's drops (icon, hotkeys) run.
pub fn pump_for(seconds: f64) {
    PUMP_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), true);
        let timer = SetTimer(None, 0, (seconds * 1000.0) as u32, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_TIMER && msg.hwnd.is_invalid() && msg.wParam.0 == timer {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = KillTimer(None, timer);
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), false);
    }
}

pub fn probe(args: &[String]) -> Result<Value> {
    if args.first().map(String::as_str) != Some("show") {
        return Err(Error::msg("Usage: tray show [seconds]   (shows the icon; left/right-click it)"));
    }
    let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(15.0);
    let icon = app_icon_path().ok_or_else(|| Error::msg("build\\icon.ico not found near the exe."))?;
    let log = Rc::new(RefCell::new(Vec::<Value>::new()));
    let hwnd = Rc::new(Cell::new(HWND::default()));
    let tray: Rc<RefCell<Option<Tray>>> = Rc::default();
    let checked = Rc::new(Cell::new(false));
    let (l, h, t, c) = (log.clone(), hwnd.clone(), tray.clone(), checked.clone());
    let window = MsgWindow::new("LaunchpadProbe-tray", move |e| {
        eprintln!("{e:?}");
        l.borrow_mut().push(json!(format!("{e:?}")));
        match e {
            Event::TaskbarCreated => {
                let ok = t.borrow().as_ref().map(|t| t.readd().is_ok());
                l.borrow_mut().push(json!({ "readd": ok }));
            }
            Event::TrayMenu { x, y } => {
                let items = [
                    MenuItem::Item(1, "Show Launchpad"),
                    MenuItem::Separator,
                    MenuItem::Check(2, "Start with Windows (probe: does nothing)", c.get()),
                    MenuItem::Separator,
                    MenuItem::Item(3, "Quit"),
                ];
                let picked = popup_menu(h.get(), x, y, &items);
                eprintln!("menu: {picked:?}");
                l.borrow_mut().push(json!({ "menu": picked }));
                match picked {
                    Some(2) => c.set(!c.get()),
                    Some(3) => unsafe { PostQuitMessage(0) },
                    _ => {}
                }
            }
            _ => {}
        }
    })?;
    hwnd.set(window.hwnd());
    let mut t = Tray::new(&window, &icon, "Launchpad")?;
    t.set_tooltip("Launchpad (probe)")?;
    *tray.borrow_mut() = Some(t);
    let rect = tray.borrow().as_ref().and_then(Tray::rect);
    eprintln!("Tray icon shown for {secs} s at {rect:?}; click it, right-click for the menu, Ctrl+C to stop.");
    if args.iter().any(|a| a == "--self-taskbar-created") {
        // Exercises the re-add path without restarting Explorer.
        let _ = unsafe { PostMessageW(Some(window.hwnd()), taskbar_created(), WPARAM(0), LPARAM(0)) };
    }
    pump_for(secs);
    tray.borrow_mut().take(); // icon removed here
    let events = log.borrow().clone();
    Ok(json!({ "iconRect": rect, "events": events, "checked": checked.get() }))
}
