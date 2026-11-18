//! Launchpad's native build. Phase 2 (RUST_PORT_PLAN.md §5): the Windows
//! layer, driven from `--probe` until the UI arrives in Phase 3.
//!
//! `launchpad --probe <area> [action] [args…]` prints JSON on stdout and
//! exits 0, or prints `{"error": "…"}` and exits 1. With no action a probe
//! only reads; anything that changes the machine needs an explicit action.

// shortcut: console subsystem so probe output shows in a terminal. Phase 3
// switches to the windows subsystem and attaches to the parent console for --probe.
mod win;

use serde_json::{json, Value};

type Probe = fn(&[String]) -> launchpad_core::Result<Value>;

const PROBES: &[(&str, Probe, &str)] = &[
    ("audio", win::audio::probe, "master volume, output devices, per-app sessions"),
    ("wifi", win::wifi::probe, "Wi-Fi networks and the current connection"),
    ("bluetooth", win::bluetooth::probe, "paired Bluetooth devices"),
    ("drives", win::drives::probe, "removable USB drives"),
    ("power", win::power::probe, "shutdown, restart, sleep (actions need --yes)"),
    ("windows", win::windowmgr::probe, "open windows, foreground, placing, fullscreen, events"),
    ("displays", win::display::probe, "monitors, work areas and scaling"),
    ("appbar", win::appbar::probe, "docking a test bar at the top of the screen"),
    ("image", win::image::probe, "exe icons, thumbnails, decode and resize"),
    ("http", win::http::probe, "GET a URL"),
    ("sha512", win::crypto::probe, "SHA-512 of a file"),
    ("launch", win::launch::probe, "shortcut resolving and launching"),
    ("services", win::services::probe, "core's traits run for real: favicon, update check"),
    ("hotkey", win::hotkey::probe, "global hotkeys"),
    ("tray", win::tray::probe, "the notification-area icon and its menu"),
    ("instance", win::instance::probe, "single-instance lock"),
    ("autostart", win::autostart::probe, "start with Windows"),
    ("dialogs", win::dialogs::probe, "file and folder pickers"),
    ("notify", win::notify::probe, "system notifications"),
];

fn main() {
    win::init_process();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--probe") => std::process::exit(run_probe(&args[1..])),
        _ => {
            eprintln!("Launchpad {}: no UI yet (Phase 3). Try --probe.", env!("CARGO_PKG_VERSION"));
            for (name, _, what) in PROBES {
                eprintln!("  --probe {name:<10} {what}");
            }
        }
    }
}

fn run_probe(args: &[String]) -> i32 {
    let Some(area) = args.first() else {
        print(&json!({ "areas": PROBES.iter().map(|(n, _, w)| json!({ "area": n, "what": w })).collect::<Vec<_>>() }));
        return 0;
    };
    let Some((_, probe, _)) = PROBES.iter().find(|(n, _, _)| n == area) else {
        print(&json!({ "error": format!("No probe called {area}.") }));
        return 1;
    };
    match probe(&args[1..]) {
        Ok(v) => {
            print(&v);
            0
        }
        Err(e) => {
            print(&json!({ "error": e.to_string() }));
            1
        }
    }
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}
