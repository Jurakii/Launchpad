//! System notifications (Electron's `Notification`), as a balloon on the
//! tray icon: Windows 10/11 shows it as a toast, with no AppUserModelID or
//! Start-menu shortcut needed. Clicks come back as `Event::BalloonClick`.
//! The toast's header names the exe's FileDescription (Phase 6 sets it).
//!
//! Real toasts (`Windows.UI.Notifications`) would additionally need: an
//! AppUserModelID set on the process and on a Start-menu shortcut (the
//! installer's), or a registered COM activator for clicks after exit; the
//! WinRT features in Cargo.toml; and XML payloads. Not needed for the two
//! messages 1.2 sends (update available, organizer errors).
use super::tray::{self, copy_into, Event, MsgWindow, Tray};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::UI::Shell::{NIF_INFO, NIIF_RESPECT_QUIET_TIME, NIIF_USER, NIM_MODIFY};

/// Shows `title` / `body` from the tray icon. Replaces any balloon still up.
pub fn show(tray: &Tray, title: &str, body: &str) -> Result<()> {
    let mut d = tray.data();
    d.uFlags |= NIF_INFO;
    d.dwInfoFlags = NIIF_USER | NIIF_RESPECT_QUIET_TIME; // NIIF_USER: the tray's own icon
    copy_into(&mut d.szInfoTitle, title);
    // An empty body removes the balloon instead of showing one.
    copy_into(&mut d.szInfo, if body.is_empty() { " " } else { body });
    tray.notify(NIM_MODIFY, &d)
}

pub fn probe(args: &[String]) -> Result<Value> {
    if args.first().map(String::as_str) != Some("show") {
        return Err(Error::msg("Usage: notify show --title T --body B [--seconds N, default 8]"));
    }
    let title = super::arg(args, "--title").unwrap_or("Launchpad");
    let body = super::arg(args, "--body").unwrap_or("Probe notification.");
    let secs: f64 = super::arg(args, "--seconds").and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let icon = tray::app_icon_path().ok_or_else(|| Error::msg("build\\icon.ico not found near the exe."))?;
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l = log.clone();
    let window = MsgWindow::new("LaunchpadProbe-notify", move |e: Event| {
        eprintln!("{e:?}");
        l.borrow_mut().push(format!("{e:?}"));
    })?;
    let tray = Tray::new(&window, &icon, "Launchpad (probe)")?;
    show(&tray, title, body)?;
    tray::pump_for(secs);
    drop(tray);
    let events = log.borrow().clone();
    Ok(json!({ "title": title, "body": body, "events": events }))
}
