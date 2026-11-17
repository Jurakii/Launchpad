// Phase 0 spike: can Slint drive Launchpad's always-on shell windows (top panel
// + click-through zone overlay) with per-pixel alpha and the Win32 styles they need?
//   spike-shell-windows --panel|--overlay|--both [--seconds N] [--renderer software|femtovg]
//   spike-shell-windows --remove-appbar <hwnd>     (recovery after a hard kill)
//   --panic: panic after 3 s, to prove the panic hook gives the work area back.
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

slint::slint! {
    #[edition = "1.19"] // NB: inline macro can't lex colours like #8e…: rustc reads a float exponent
    export component Panel inherits Window {
        no-frame: true;
        always-on-top: true;
        title: "spike-panel";
        background: #10121c99; // 60% alpha: the desktop must show through
        default-font-size: 13px;
        in property <string> clock: "--:--:--";
        in-out property <int> counter: 0;
        callback bumped(int);

        Rectangle {
            x: 8px; y: 3px; width: 330px; height: 26px;
            border-radius: 13px;
            background: #2b2d42;
            HorizontalLayout {
                padding-left: 12px; padding-right: 12px; spacing: 12px;
                Text { text: "Notepad — untitled"; color: white; vertical-alignment: center; }
                Text { text: "日本語 Café"; color: #ffd166; vertical-alignment: center; }
                Text { text: root.clock; color: #8fcae6; vertical-alignment: center; horizontal-alignment: right; }
            }
        }
        for b[i] in [{ t: "+1", d: 1 }, { t: "-1", d: -1 }]: Rectangle {
            x: 350px + i * 66px; y: 3px; width: 60px; height: 26px;
            border-radius: 6px;
            background: ta.pressed ? #e76f51 : ta.has-hover ? #3a86ff : #264653;
            Text { text: b.t; color: white; }
            ta := TouchArea {
                clicked => { root.counter += b.d; root.bumped(root.counter); }
            }
        }
        Text { x: 490px; y: 0; height: 100%; text: "count: " + root.counter; color: white; vertical-alignment: center; }
    }

    export component Overlay inherits Window {
        no-frame: true;
        always-on-top: true;
        title: "spike-overlay";
        background: transparent;
        in property <int> active: 1;
        in property <color> accent: #3a86ff;
        for i in 3: Rectangle {
            x: parent.width * (0.04 + i * 0.32); y: parent.height * 0.15;
            width: parent.width * 0.28; height: parent.height * 0.6;
            border-radius: 14px; border-width: 3px; border-color: root.accent;
            background: root.accent.with-alpha(i == root.active ? 0.45 : 0.15);
        }
    }
}

const BAR_H: i32 = 32;
static APPBAR: AtomicIsize = AtomicIsize::new(0);

fn work_area() -> RECT {
    let mut r = RECT::default();
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut _ as _), Default::default()).ok() };
    r
}

fn log_work_area(when: &str) {
    let r = work_area();
    println!("workarea {when}: {},{},{},{}", r.left, r.top, r.right, r.bottom);
}

fn appbar_data(hwnd: isize) -> APPBARDATA {
    APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: HWND(hwnd as _), ..Default::default() }
}

// Safe from any thread and more than once: Explorer only needs the handle value.
fn remove_appbar() {
    let h = APPBAR.swap(0, Ordering::SeqCst);
    if h != 0 {
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut appbar_data(h)) };
    }
}

unsafe extern "system" fn on_console_ctrl(_: u32) -> BOOL {
    remove_appbar();
    false.into() // let the default handler end the process
}

fn hwnd_of(w: &slint::Window) -> HWND {
    match w.window_handle().window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => HWND(h.hwnd.get() as _),
        other => panic!("no HWND yet: {other:?}"),
    }
}

fn add_ex_style(hwnd: HWND, bits: WINDOW_EX_STYLE) {
    unsafe {
        // winit sets WS_EX_APPWINDOW, which would put a tool window back in Alt+Tab.
        let ex = (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | bits.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        println!("hwnd {:#x} exstyle {:#x}", hwnd.0 as usize, GetWindowLongPtrW(hwnd, GWL_EXSTYLE));
    }
}

fn place(hwnd: HWND, r: RECT) {
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE | SWP_FRAMECHANGED).ok();
    }
}

fn dock_panel(hwnd: HWND) {
    add_ex_style(hwnd, WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW);
    log_work_area("before");
    let h = hwnd.0 as isize;
    unsafe {
        let mut d = appbar_data(h);
        // shortcut: the callback message goes to winit's wndproc, which drops it;
        // the real app must subclass the window and re-run SETPOS on ABN_POSCHANGED.
        d.uCallbackMessage = WM_APP + 1;
        if SHAppBarMessage(ABM_NEW, &mut d) == 0 {
            println!("ABM_NEW failed");
            return;
        }
        APPBAR.store(h, Ordering::SeqCst);
        d.uEdge = ABE_TOP;
        d.rc = RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: BAR_H };
        SHAppBarMessage(ABM_QUERYPOS, &mut d);
        d.rc.bottom = d.rc.top + BAR_H;
        SHAppBarMessage(ABM_SETPOS, &mut d);
        place(hwnd, d.rc);
    }
}

fn style_overlay(o: &Overlay) {
    let hwnd = hwnd_of(o.window());
    add_ex_style(hwnd, WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW);
    // A layered window shows nothing until it's given its attributes.
    unsafe { SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA).ok() };
    place(hwnd, work_area());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();

    if let Some(h) = value("--remove-appbar") {
        let h = h.trim_start_matches("0x");
        APPBAR.store(isize::from_str_radix(h, 16).expect("hex hwnd"), Ordering::SeqCst);
        remove_appbar();
        std::thread::sleep(Duration::from_millis(300));
        return log_work_area("after removal");
    }
    let both = flag("--both");
    let (want_panel, want_overlay) = (both || flag("--panel"), both || flag("--overlay"));
    assert!(want_panel || want_overlay, "pass --panel, --overlay or --both");
    let seconds: u64 = value("--seconds").map_or(20, |s| s.parse().expect("--seconds N"));

    std::panic::set_hook(Box::new(|info| {
        remove_appbar();
        eprintln!("{info}");
    }));
    unsafe { SetConsoleCtrlHandler(Some(on_console_ctrl), true).ok() };

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name(value("--renderer").unwrap_or("software".into()))
        // Styles set after show() are too late for these two: the window would take
        // the foreground once and flash a taskbar button.
        .with_winit_window_attributes_hook(|a| a.with_active(false).with_skip_taskbar(true))
        .select()
        .expect("backend");

    println!("foreground at start: {:#x}", unsafe { GetForegroundWindow() }.0 as usize);
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;

    let panel = want_panel.then(|| Panel::new().unwrap());
    let mut overlay = want_overlay.then(|| Overlay::new().unwrap());
    let mut clock_timer = None;

    if let Some(p) = &panel {
        p.on_bumped(|n| println!("counter={n}"));
        p.window().set_position(slint::PhysicalPosition::new(0, 0));
        p.window().set_size(slint::PhysicalSize::new(screen_w, BAR_H as u32));
        p.show().unwrap();
        let t = slint::Timer::default();
        let weak = p.as_weak();
        t.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
            let n = unsafe { GetLocalTime() };
            if let Some(p) = weak.upgrade() {
                p.set_clock(format!("{:02}:{:02}:{:02}", n.wHour, n.wMinute, n.wSecond).into());
            }
        });
        clock_timer = Some(t);
    }
    if let Some(o) = &overlay {
        o.window().set_size(slint::PhysicalSize::new(screen_w, 1));
        o.show().unwrap();
    }

    // The HWND only exists once the event loop has created the winit window.
    let (pw, ow) = (panel.as_ref().map(|p| p.as_weak()), overlay.as_ref().map(|o| o.as_weak()));
    slint::Timer::single_shot(Duration::from_millis(100), move || {
        if let Some(p) = pw.as_ref().and_then(|w| w.upgrade()) {
            dock_panel(hwnd_of(p.window()));
        }
        // Explorer applies the new work area asynchronously.
        slint::Timer::single_shot(Duration::from_millis(400), move || {
            log_work_area("after docking");
            if let Some(o) = ow.as_ref().and_then(|w| w.upgrade()) {
                style_overlay(&o);
            }
            println!("foreground after setup: {:#x}", unsafe { GetForegroundWindow() }.0 as usize);
        });
    });
    if flag("--panic") {
        slint::Timer::single_shot(Duration::from_secs(3), || panic!("--panic test"));
    }
    // --hide-at N: hide the overlay at N s and show it again 8 s later, to see
    // whether its backing buffer is freed and whether the HWND survives.
    if let (Some(n), Some(o)) = (value("--hide-at"), &overlay) {
        let (w1, w2) = (o.as_weak(), o.as_weak());
        let n: u64 = n.parse().expect("--hide-at N");
        slint::Timer::single_shot(Duration::from_secs(n), move || w1.unwrap().hide().unwrap());
        slint::Timer::single_shot(Duration::from_secs(n + 8), move || {
            w2.unwrap().show().unwrap();
            slint::Timer::single_shot(Duration::from_millis(100), move || style_overlay(&w2.unwrap()));
        });
    }
    // --drop-at N: destroy the overlay component at N s and build a new one 8 s later.
    if let (Some(n), Some(o)) = (value("--drop-at"), overlay.take()) {
        let n: u64 = n.parse().expect("--drop-at N");
        slint::Timer::single_shot(Duration::from_secs(n), move || {
            // drop alone isn't enough: a shown window keeps its component alive
            o.hide().unwrap();
            drop(o);
        });
        slint::Timer::single_shot(Duration::from_secs(n + 8), move || {
            let o = Overlay::new().unwrap();
            o.window().set_size(slint::PhysicalSize::new(screen_w, 1));
            o.show().unwrap();
            slint::Timer::single_shot(Duration::from_millis(100), move || {
                style_overlay(&o);
                std::mem::forget(o); // spike: lives until exit
            });
        });
    }
    slint::Timer::single_shot(Duration::from_secs(seconds), || slint::quit_event_loop().unwrap());

    // Not run_event_loop(): that one quits when the last window is hidden.
    slint::run_event_loop_until_quit().unwrap();
    drop(clock_timer);
    remove_appbar();
    std::thread::sleep(Duration::from_millis(400));
    log_work_area("after removal");
}
