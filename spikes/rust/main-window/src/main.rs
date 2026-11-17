// Phase 0 spike: Launchpad main window in Slint. Throwaway.
#![windows_subsystem = "windows"]

use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let has = |flag: &str| std::env::args().any(|a| a == flag);
    // Loaded from disk at runtime, like the real app's wallpaper and icons.
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let load = |name: String| Image::load_from_path(&assets.join(name)).expect("asset");

    let ui = MainWindow::new()?;
    ui.set_wallpaper(load("wallpaper.jpg".into()));

    let icons: Vec<Image> = (0..20).map(|i| load(format!("icon{i:02}.png"))).collect();
    let special = ["日本語のタイトル", "Café Zürich", "Привет", "Game 🎮"];
    let tiles: Vec<TileData> = (0..200usize)
        .map(|i| TileData {
            name: special.get(i.wrapping_sub(1)).map_or_else(|| format!("Application {i}"), |s| s.to_string()).into(),
            icon: icons[i % icons.len()].clone(),
            col: (i % 10) as i32,
            row: (i / 10) as i32,
        })
        .collect();
    ui.set_tiles(ModelRc::new(VecModel::from(tiles)));

    // shortcut: clock is UTC (std has no local time); use GetLocalTime in the real app.
    let clock = Timer::default();
    let weak = ui.as_weak();
    let tick = move || {
        let s = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if let Some(ui) = weak.upgrade() {
            ui.set_clock(format!("{:02}:{:02}:{:02} UTC", s / 3600 % 24, s / 60 % 60, s % 60).into());
        }
    };
    tick();
    clock.start(TimerMode::Repeated, Duration::from_secs(1), tick);

    if has("--maximized") {
        ui.window().set_maximized(true);
    }

    if has("--auto-drag") || has("--auto-scroll") {
        ui.set_auto_drag(has("--auto-drag"));
        ui.set_auto_scroll(has("--auto-scroll"));
        // Counts real renders where the renderer supports a notifier, to check ticks == drawn frames.
        let renders = Rc::new(std::cell::Cell::new(0u32));
        let r = renders.clone();
        let notifier = ui.window().set_rendering_notifier(move |state, _| {
            if matches!(state, slint::RenderingState::AfterRendering) {
                r.set(r.get() + 1);
            }
        });
        // (measurement start, previous frame, intervals in ms)
        let state = Rc::new(RefCell::new((None::<Instant>, Instant::now(), Vec::<f64>::new())));
        ui.on_frame(move || {
            let mut s = state.borrow_mut();
            let now = Instant::now();
            let Some(start) = s.0 else {
                *s = (Some(now), now, Vec::new());
                return;
            };
            let dt = now.duration_since(s.1).as_secs_f64() * 1000.0;
            s.1 = now;
            s.2.push(dt);
            if now.duration_since(start) >= Duration::from_secs(5) {
                // Interval between consecutive animation ticks; each tick is followed by one redraw + present.
                let slow: Vec<String> =
                    s.2.iter().enumerate().filter(|(_, d)| **d > 16.7).map(|(i, d)| format!("#{i}:{d:.1}ms")).collect();
                let v = &mut s.2;
                v.sort_by(|a, b| a.total_cmp(b));
                let n = v.len();
                let avg = v.iter().sum::<f64>() / n as f64;
                println!("render notifier: {notifier:?} renders={}", renders.get());
                println!(
                    "auto: frames={n} avg={avg:.2}ms ({:.1} fps) median={:.2}ms p99={:.2}ms worst={:.2}ms over16.7ms={} {:?}",
                    1000.0 / avg,
                    v[n / 2],
                    v[n * 99 / 100],
                    v[n - 1],
                    slow.len(),
                    slow
                );
                let _ = slint::quit_event_loop();
            }
        });
    }

    ui.run()
}
