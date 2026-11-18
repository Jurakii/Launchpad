//! The tray widgets (ui/tray.slint): volume, USB, Bluetooth, network, clock,
//! power, and their order. Lists load on a thread when a popout opens; audio
//! and Wi-Fi changes arrive from `audio::watch` / `wifi::watch` and drive
//! changes from WM_DEVICECHANGE, all only while the window exists.
use super::{modals, post, App, MainWindow, Popouts, TrayGap, TrayRow, TraySession, TrayState};
use crate::win::{self, audio, bluetooth, drives, launch, power, wifi};
use launchpad_core::Result;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::rc::{Rc, Weak};
use std::time::Duration;

const TRAY_IDS: [&str; 6] = ["volume", "usb", "bluetooth", "network", "clock", "power"];
const ORDER_KEY: &str = "launchpad-tray-order";
const GAP: f32 = 6.0;
const BUTTON: f32 = 36.0;

/// Everything that lives exactly as long as one window. Held by a callback of
/// that window, so it is dropped (unregistering the watches, stopping the
/// clock) when the window is.
struct Live {
    _audio: Option<audio::Watch>,
    _wifi: Option<wifi::Watch>,
    _clock: Rc<Timer>,
}

/// Wires the global's callbacks on a newly made window.
pub fn bind(app: &Rc<App>, w: &MainWindow) {
    close_popouts_from_outside(w);
    let t = w.global::<TrayState>();
    push_order(app, w);
    set_clock(w);
    if let Ok(m) = audio::master() {
        t.set_volume(m.volume as f32);
        t.set_muted(m.muted);
    }
    bg(online, |w, on| w.global::<TrayState>().set_online(on));

    t.on_opened(|id| opened(id.as_str()));
    t.on_set_volume(|v| log("Set volume", audio::set_volume(v.clamp(0, 100) as u32)));
    t.on_toggle_mute(|| {
        with_window(|w| {
            let t = w.global::<TrayState>();
            log("Set mute", audio::set_muted(!t.get_muted()));
            if let Ok(m) = audio::master() {
                t.set_volume(m.volume as f32);
                t.set_muted(m.muted);
            }
        });
    });
    t.on_pick_device(|i| {
        with_window(|w| {
            let Some(id) = w.global::<TrayState>().get_device_ids().row_data(i as usize) else {
                return;
            };
            // 1.2 ignored the result; a failure shows as the old device staying selected.
            bg(
                move || act("set_default_device", || audio::set_default_device(&id)),
                |_, _| load_devices(),
            );
        });
    });
    t.on_set_session_volume(|pid, v| {
        log("Set app volume", audio::set_session_volume(pid as u32, v.clamp(0, 100) as u32));
        update_session(pid, |s| s.volume = v);
    });
    t.on_toggle_session_mute(|pid| {
        let mut now = false;
        update_session(pid, |s| {
            s.muted = !s.muted;
            now = s.muted;
        });
        log("Set app mute", audio::set_session_muted(pid as u32, now));
    });
    t.on_row_action(|kind, row| row_action(kind.as_str(), row));
    t.on_settings_link(|which| {
        let r = if which == "network" {
            wifi::open_settings()
        } else {
            launch::open_external("ms-settings:bluetooth")
        };
        log("Open settings", r);
    });
    t.on_power(|kind| power_action(kind.to_string()));
    t.on_gap(|pos, clock| {
        let order = with_window(|w| model_strings(&w.global::<TrayState>().get_order())).unwrap_or_default();
        let (index, offset) = gap(&order, clock, pos);
        TrayGap {
            index: index as i32,
            offset,
        }
    });
    t.on_dropped(|id, index| {
        super::with(|app| {
            let Some(w) = window(app) else { return };
            let shown = model_strings(&w.global::<TrayState>().get_order());
            if let Some(mut order) = reorder(&shown, &id, index.max(0) as usize) {
                if mirrored(app) {
                    order.reverse();
                }
                app.set_pref(ORDER_KEY, Some(&serde_json::to_string(&order).unwrap_or_default()));
                push_order(app, &w);
            }
        });
    });

    let audio = audio::watch(|ev| {
        post(move |_| match ev {
            audio::AudioEvent::Volume(m) => {
                with_window(|w| {
                    let t = w.global::<TrayState>();
                    t.set_volume(m.volume as f32);
                    t.set_muted(m.muted);
                });
            }
            audio::AudioEvent::DefaultDevice(_) | audio::AudioEvent::DevicesChanged(_) => {
                if is_open("volume") {
                    load_devices();
                }
            }
        })
    });
    let wifi = wifi::watch(|ev| {
        post(move |_| {
            if ev != wifi::Event::ScanComplete {
                bg(online, |w, on| w.global::<TrayState>().set_online(on));
            }
            if is_open("network") {
                load_networks();
            }
        })
    });
    let clock = Rc::new(Timer::default());
    schedule_clock(&clock);
    let live = Live {
        _audio: audio.map_err(|e| eprintln!("Audio watch: {e}")).ok(),
        _wifi: wifi.ok(),
        _clock: clock,
    };
    t.on_keepalive(move || {
        let _ = &live;
    });
}

/// Pushes the current state into the window. Only the order: the lists are
/// loaded when their popout opens, and re-setting them would close it.
pub fn refresh(app: &App, w: &MainWindow) {
    push_order(app, w);
}

/// Taskbar popouts are plain elements, so a click outside one reaches
/// whatever is there (as in 1.2) and also closes it; Esc closes it too.
/// "search" closes itself (taskbar.slint).
fn close_popouts_from_outside(w: &MainWindow) {
    use slint::winit_030::winit::event::{ElementState, WindowEvent};
    use slint::winit_030::winit::keyboard::{Key, NamedKey};
    use slint::winit_030::{EventResult, WinitWindowAccessor};
    let mut cursor = (0.0f32, 0.0f32);
    w.window().on_winit_window_event(move |win, ev| {
        match ev {
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(f64::from(win.scale_factor()));
                cursor = (p.x, p.y);
            }
            // On release: a tray or Recent button toggles in `clicked`, which has
            // run by the time `later` does, so a changed value is left alone.
            WindowEvent::MouseInput { state: ElementState::Released, .. } => {
                let (x, y) = cursor;
                let before = with_window(|w| w.global::<Popouts>().get_open()).unwrap_or_default();
                if !before.is_empty() && before != "search" {
                    super::later(move |app| {
                        let Some(w) = window(app) else { return };
                        let p = w.global::<Popouts>();
                        let inside = x >= p.get_panel_x()
                            && x < p.get_panel_x() + p.get_panel_w()
                            && y >= p.get_panel_y()
                            && y < p.get_panel_y() + p.get_panel_h();
                        if p.get_open() == before && !inside {
                            p.set_open("".into());
                        }
                    });
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && event.logical_key == Key::Named(NamedKey::Escape) => {
                with_window(|w| {
                    let p = w.global::<Popouts>();
                    if p.get_open() != "search" {
                        p.set_open("".into());
                    }
                });
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

// ---- Order ---------------------------------------------------------------

/// The saved order if it is exactly the six ids in some order (1.2's check), else the default.
fn parse_order(saved: Option<&str>) -> Vec<String> {
    let parsed: Option<Vec<String>> = saved.and_then(|s| serde_json::from_str(s).ok());
    match parsed {
        Some(v) if v.len() == TRAY_IDS.len() && TRAY_IDS.iter().all(|id| v.iter().any(|s| s == id)) => v,
        _ => TRAY_IDS.iter().map(|s| s.to_string()).collect(),
    }
}

/// `order` with `dragged` moved to insert gap `at` (1.2's drop handler).
fn reorder(order: &[String], dragged: &str, at: usize) -> Option<Vec<String>> {
    let from = order.iter().position(|s| s == dragged)?;
    let mut v = order.to_vec();
    let id = v.remove(from);
    let at = if from < at { at - 1 } else { at };
    v.insert(at.min(v.len()), id);
    Some(v)
}

/// findInsertGap: the first widget whose middle is past `pos`, and where its
/// slot starts; after the last one otherwise.
fn gap(order: &[String], clock: f32, pos: f32) -> (usize, f32) {
    let mut start = 0.0;
    for (i, id) in order.iter().enumerate() {
        let ext = if id == "clock" { clock } else { BUTTON };
        if pos < start + ext / 2.0 {
            return (i, start);
        }
        start += ext + GAP;
    }
    (order.len(), (start - GAP).max(0.0))
}

fn mirrored(app: &App) -> bool {
    app.pref("launchpad-taskbar-position").as_deref() == Some("right")
}

fn push_order(app: &App, w: &MainWindow) {
    let mut order = parse_order(app.pref(ORDER_KEY).as_deref());
    if mirrored(app) {
        order.reverse();
    }
    let t = w.global::<TrayState>();
    // Same order: leave the model alone, replacing it would close an open popout.
    if model_strings(&t.get_order()) != order {
        t.set_order(strings(order));
    }
}

// ---- Popouts and lists ---------------------------------------------------

fn opened(id: &str) {
    match id {
        "volume" => {
            with_window(|w| {
                let t = w.global::<TrayState>();
                t.set_sessions_scanning(true);
                if let Ok(m) = audio::master() {
                    t.set_volume(m.volume as f32);
                    t.set_muted(m.muted);
                }
            });
            load_devices();
            bg(
                || audio::sessions().unwrap_or_default(),
                |w, list| {
                    let t = w.global::<TrayState>();
                    t.set_sessions_scanning(false);
                    let rows: Vec<_> = list
                        .into_iter()
                        .map(|s| TraySession {
                            pid: s.pid as i32,
                            name: s.name.into(),
                            volume: s.volume as i32,
                            muted: s.muted,
                        })
                        .collect();
                    t.set_sessions(ModelRc::new(VecModel::from(rows)));
                },
            );
        }
        "usb" => {
            watch_device_changes();
            with_window(|w| {
                let t = w.global::<TrayState>();
                t.set_usb_scanning(true);
                t.set_usb(ModelRc::default());
            });
            load_usb();
        }
        "bluetooth" => {
            with_window(|w| {
                let t = w.global::<TrayState>();
                t.set_bluetooth_scanning(true);
                t.set_bluetooth(ModelRc::default());
            });
            bg(
                || bluetooth::list().unwrap_or_default(),
                |w, list| {
                    let t = w.global::<TrayState>();
                    t.set_bluetooth_scanning(false);
                    let rows = list.into_iter().map(|d| TrayRow {
                        title: d.name.unwrap_or_default().into(),
                        detail: if d.connected { "Connected" } else { "Paired, not connected" }.into(),
                        signal: -1,
                        ..Default::default()
                    });
                    t.set_bluetooth(ModelRc::new(VecModel::from(rows.collect::<Vec<_>>())));
                },
            );
        }
        "network" => {
            with_window(|w| {
                let t = w.global::<TrayState>();
                t.set_networks_scanning(true);
                t.set_networks(ModelRc::default());
            });
            load_networks();
            // A fresh scan; its ScanComplete reloads the list (wifi::watch).
            bg(wifi::scan, |_, _| {});
        }
        _ => {}
    }
}

fn load_devices() {
    bg(
        || audio::devices().unwrap_or_default(),
        |w, list| {
            let t = w.global::<TrayState>();
            t.set_device_index(list.iter().position(|d| d.is_default).map_or(-1, |i| i as i32));
            t.set_devices(strings(list.iter().map(|d| d.name.clone())));
            t.set_device_ids(strings(list.into_iter().map(|d| d.id)));
        },
    );
}

fn format_bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1024f64.powi(3);
    if gb >= 1024.0 {
        format!("{:.1} TB", gb / 1024.0)
    } else {
        format!("{gb:.1} GB")
    }
}

fn load_usb() {
    bg(
        || drives::list().unwrap_or_default(),
        |w, list| {
            let t = w.global::<TrayState>();
            t.set_usb_scanning(false);
            let rows = list.into_iter().map(|d| TrayRow {
                title: if d.label.is_empty() {
                    d.drive_letter.clone()
                } else {
                    format!("{} ({})", d.label, d.drive_letter)
                }
                .into(),
                detail: format!("{} free of {}", format_bytes(d.free_space), format_bytes(d.size)).into(),
                key: d.drive_letter.into(),
                enabled: true,
                signal: -1,
                ..Default::default()
            });
            t.set_usb(ModelRc::new(VecModel::from(rows.collect::<Vec<_>>())));
        },
    );
}

fn network_row(n: wifi::Network) -> TrayRow {
    let detail = if n.connected {
        "Connected".to_string()
    } else if n.saved {
        format!("{} · Saved", n.auth)
    } else {
        n.auth.to_string()
    };
    TrayRow {
        key: n.profile.clone().unwrap_or_else(|| n.ssid.clone()).into(),
        title: n.ssid.into(),
        detail: detail.into(),
        action: if n.connected { "Disconnect" } else { "Connect" }.into(),
        enabled: n.connected || n.saved,
        danger: n.connected,
        tip: if n.connected || n.saved {
            ""
        } else {
            "No saved password for this network yet - connect once via Windows Settings first"
        }
        .into(),
        signal: n.signal as i32,
    }
}

fn load_networks() {
    bg(
        || wifi::list().unwrap_or_default(),
        |w, list| {
            let t = w.global::<TrayState>();
            t.set_networks_scanning(false);
            t.set_networks(ModelRc::new(VecModel::from(list.into_iter().map(network_row).collect::<Vec<_>>())));
        },
    );
}

/// Greys out the row's button while its action runs, as 1.2 disabled it.
fn disable_row(kind: &str, key: &str) {
    with_window(|w| {
        let t = w.global::<TrayState>();
        let m = if kind == "usb" { t.get_usb() } else { t.get_networks() };
        for i in 0..m.row_count() {
            if let Some(mut r) = m.row_data(i).filter(|r| r.key == key) {
                r.enabled = false;
                m.set_row_data(i, r);
            }
        }
    });
}

fn row_action(kind: &str, row: TrayRow) {
    disable_row(kind, &row.key);
    if kind == "usb" {
        let letter = row.key.to_string();
        bg(
            {
                let letter = letter.clone();
                move || act("eject", || drives::eject(&letter))
            },
            move |_, r| {
                if let Err(e) = r {
                    alert(format!("Couldn't eject {letter}\n{e}"));
                }
                load_usb();
            },
        );
    } else if row.action == "Disconnect" {
        bg(
            || act("wifi disconnect", wifi::disconnect),
            |_, r| {
                if let Err(e) = r {
                    alert(format!("Couldn't disconnect:\n{e}"));
                }
                load_networks();
            },
        );
    } else {
        let (profile, ssid) = (row.key.to_string(), row.title.to_string());
        bg(
            move || act("wifi connect", || wifi::connect(&profile)),
            move |_, r| {
                if let Err(e) = r {
                    alert(format!("Couldn't connect to {ssid}:\n{e}"));
                }
                load_networks();
            },
        );
    }
}

fn power_action(kind: String) {
    with_window(|w| w.global::<Popouts>().set_open("".into()));
    let run = move |kind: &str| {
        let kind = kind.to_string();
        // Sleep blocks until the PC wakes, so all three run on a thread.
        bg(
            move || match kind.as_str() {
                "sleep" => act("sleep", power::sleep),
                "restart" => act("restart", power::restart),
                _ => act("shutdown", power::shutdown),
            },
            |_, r| log("Power", r),
        );
    };
    let question = match kind.as_str() {
        "restart" => "Restart this PC now? Unsaved work in other apps will be lost.",
        "shutdown" => "Shut down this PC now? Unsaved work in other apps will be lost.",
        // 1.2 slept without asking.
        _ => return run(&kind),
    };
    super::with(|app| modals::confirm(app, question, "OK", false, move |_| run(&kind)));
}

// ---- Clock ---------------------------------------------------------------

fn set_clock(w: &MainWindow) {
    use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, DATE_SHORTDATE, TIME_NOSECONDS};
    let (mut time, mut date) = ([0u16; 80], [0u16; 80]);
    // toLocaleTimeString({hour:'numeric', minute:'2-digit'}) / toLocaleDateString(): the user's short formats.
    let tn = unsafe { GetTimeFormatEx(None, TIME_NOSECONDS, None, None, Some(&mut time)) };
    let dn = unsafe { GetDateFormatEx(None, DATE_SHORTDATE, None, None, Some(&mut date), None) };
    let t = w.global::<TrayState>();
    t.set_clock_time(win::from_wide(&time[..tn.max(0) as usize]).into());
    t.set_clock_date(win::from_wide(&date[..dn.max(0) as usize]).into());
}

/// Ticks just after each minute turns, while the timer (and so the window) lives.
fn schedule_clock(timer: &Rc<Timer>) {
    // Time zones are whole minutes, so UTC's place in the minute is local time's.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ms = 60_000 - (now.as_millis() % 60_000) as u64 + 20;
    let weak: Weak<Timer> = Rc::downgrade(timer);
    timer.start(slint::TimerMode::SingleShot, Duration::from_millis(ms), move || {
        with_window(|w| set_clock(&w));
        if let Some(t) = weak.upgrade() {
            schedule_clock(&t);
        }
    });
}

// ---- Network status --------------------------------------------------------

/// navigator.onLine, near enough: some adapter that is up, not loopback or a
/// tunnel, with a gateway.
fn online() -> bool {
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER | GAA_FLAG_INCLUDE_GATEWAYS;
    let mut size = 16 * 1024u32;
    for _ in 0..3 {
        let mut buf = vec![0u64; size as usize / 8 + 1]; // 8-aligned for IP_ADAPTER_ADDRESSES_LH
        let p = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        match unsafe { GetAdaptersAddresses(0, flags, None, Some(p), &mut size) } {
            0 => {
                let mut a = p as *const IP_ADAPTER_ADDRESSES_LH;
                while let Some(ad) = unsafe { a.as_ref() } {
                    if ad.OperStatus == IfOperStatusUp && ad.IfType != 24 && ad.IfType != 131 && !ad.FirstGatewayAddress.is_null() {
                        return true;
                    }
                    a = ad.Next;
                }
                return false;
            }
            111 => continue,  // ERROR_BUFFER_OVERFLOW: `size` now holds what it needs
            _ => return true, // can't tell: don't claim offline
        }
    }
    true
}

// ---- Drive changes ---------------------------------------------------------

const SUBCLASS_ID: usize = 0x7472_6179; // "tray"

/// WM_DEVICECHANGE reaches the main window (top level) only; winit drops it,
/// so the window is subclassed once its HWND exists (the first USB popout).
fn watch_device_changes() {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let Some(hwnd) = super::with(|app| app.hwnd()).flatten() else {
        return;
    };
    // Same proc and id again just updates it, so this is safe to repeat.
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{WM_DEVICECHANGE, WM_NCDESTROY};
    if msg == WM_DEVICECHANGE && unsafe { drives::drives_may_have_changed(wparam, lparam) } {
        post(|_| {
            if is_open("usb") {
                load_usb();
            }
        });
    } else if msg == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

// ---- Helpers ---------------------------------------------------------------

fn window(app: &App) -> Option<MainWindow> {
    app.window.borrow().as_ref().map(|w| w.clone_strong())
}

fn with_window<R>(f: impl FnOnce(MainWindow) -> R) -> Option<R> {
    super::with(|app| window(app)).flatten().map(f)
}

fn is_open(id: &str) -> bool {
    with_window(|w| w.global::<Popouts>().get_open() == id).unwrap_or(false)
}

/// `work` on a thread (with COM), then `done` on the UI thread if a window is still up.
fn bg<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(&MainWindow, T) + Send + 'static) {
    std::thread::spawn(move || {
        let r = {
            let _com = win::Com::init();
            work()
        };
        post(move |app| {
            if let Some(w) = window(app) {
                done(&w, r);
            }
        });
    });
}

/// A call that changes the PC. A debug build with LAUNCHPAD_DRY_RUN set only
/// logs it, so the UI can be tested up to the call.
fn act(what: &str, f: impl FnOnce() -> Result<()>) -> Result<()> {
    if cfg!(debug_assertions) && std::env::var_os("LAUNCHPAD_DRY_RUN").is_some() {
        eprintln!("dry run: {what}");
        return Ok(());
    }
    f()
}

fn log(what: &str, r: Result<()>) {
    if let Err(e) = r {
        eprintln!("{what}: {e}");
    }
}

fn alert(msg: String) {
    super::with(|app| modals::alert(app, &msg));
}

fn update_session(pid: i32, f: impl FnOnce(&mut TraySession)) {
    with_window(|w| {
        let m = w.global::<TrayState>().get_sessions();
        if let Some(i) = (0..m.row_count()).find(|&i| m.row_data(i).is_some_and(|s| s.pid == pid)) {
            let mut s = m.row_data(i).unwrap_or_default();
            f(&mut s);
            m.set_row_data(i, s);
        }
    });
}

fn strings(v: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()))
}

fn model_strings(m: &ModelRc<SharedString>) -> Vec<String> {
    m.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn order_validates_like_1_2() {
        let def = ids(&TRAY_IDS);
        assert_eq!(parse_order(None), def);
        assert_eq!(parse_order(Some("not json")), def);
        assert_eq!(parse_order(Some(r#"["volume","usb"]"#)), def);
        assert_eq!(
            parse_order(Some(r#"["volume","volume","bluetooth","network","clock","power"]"#)),
            def
        );
        let custom = r#"["power","clock","network","bluetooth","usb","volume"]"#;
        assert_eq!(serde_json::to_string(&parse_order(Some(custom))).unwrap(), custom);
    }

    #[test]
    fn reorder_and_gap() {
        let o = ids(&TRAY_IDS);
        // Drag volume (0) to the gap before network (3): lands at 2.
        assert_eq!(
            reorder(&o, "volume", 3).unwrap(),
            ids(&["usb", "bluetooth", "volume", "network", "clock", "power"])
        );
        assert_eq!(reorder(&o, "power", 0).unwrap()[0], "power");
        assert_eq!(reorder(&o, "volume", 6).unwrap()[5], "volume");
        assert!(reorder(&o, "nope", 0).is_none());
        assert_eq!(gap(&o, 60.0, 0.0), (0, 0.0));
        assert_eq!(gap(&o, 60.0, 20.0), (1, 42.0));
        // volume, usb, bluetooth, network = 4 × 42; the clock is 60 wide.
        assert_eq!(gap(&o, 60.0, 168.0 + 29.0), (4, 168.0));
        assert_eq!(gap(&o, 60.0, 168.0 + 31.0), (5, 234.0));
        assert_eq!(gap(&o, 60.0, 1000.0), (6, 270.0));
        assert_eq!(format_bytes(16 * 1024u64.pow(3)), "16.0 GB");
        assert_eq!(format_bytes(2 * 1024u64.pow(4)), "2.0 TB");
    }
}
