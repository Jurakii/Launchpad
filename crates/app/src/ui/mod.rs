//! Phase 3: the app around the main window (RUST_PORT_PLAN.md §5). One UI
//! thread runs Slint's event loop, which also pumps the hidden `MsgWindow`
//! (tray, hotkeys, single instance).
//!
//! Rules every part follows:
//! - All state is here in `App`; the window is created on show and dropped on
//!   hide, so each part's `bind` pushes state into its global and wires its
//!   callbacks every time a window is made, and `refresh` re-pushes it.
//! - Data changes go through `App::mutate`, which saves apps.json and then
//!   refreshes every part.
//! - Slow work runs on a thread and comes back with `post`. Never hold a
//!   `RefCell` borrow of `App` across a dialog, a popup menu or `post`.
pub mod grid;
pub mod icons;
pub mod images;
pub mod media;
pub mod modals;
pub mod organizer;
pub mod prefs;
pub mod settings;
pub mod shell;
pub mod taskbar;
pub mod tray;

use crate::win::{self, autostart, hotkey::Hotkey, instance, tray as wtray, windowmgr};
use launchpad_core::store::{Entry, Store};
use launchpad_core::{settings as core_settings, Error, Result};
use slint::ComponentHandle;
use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

slint::include_modules!();

pub const DEFAULT_HOTKEY: &str = "CommandOrControl+Space";
const HOTKEY_MAIN: i32 = 1;
const MENU_SHOW: u32 = 1;
const MENU_AUTOSTART: u32 = 2;
const MENU_QUIT: u32 = 3;

pub struct App {
    pub data_dir: PathBuf,
    pub store: RefCell<Store>,
    /// apps.json as loaded; written back by `mutate`.
    pub apps: RefCell<Vec<Entry>>,
    pub prefs: RefCell<prefs::Prefs>,
    pub images: images::Images,
    /// The main window while it exists (only while shown).
    pub window: RefCell<Option<MainWindow>>,
    pub(crate) msg: wtray::MsgWindow,
    pub(crate) tray_icon: RefCell<Option<wtray::Tray>>,
    pub hotkey: RefCell<Hotkey>,
    /// The app window the user was in before Launchpad came forward.
    return_focus: Cell<i64>,
    /// "Disable hotkey while another app is focused": the hook thread, while on.
    hotkey_follows_focus: RefCell<Option<windowmgr::events::Events>>,
    /// Set by `relaunch`: start again once this instance has let go of its lock.
    relaunch: Cell<bool>,
}

thread_local! {
    static APP: OnceCell<Rc<App>> = const { OnceCell::new() };
}

/// The app, on the UI thread. None before `run` has built it.
pub fn with<R>(f: impl FnOnce(&Rc<App>) -> R) -> Option<R> {
    APP.with(|a| a.get().map(f))
}

/// Runs `f` on the UI thread, from any thread.
pub fn post(f: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        with(f);
    });
}

/// Runs `f` once the current callback has returned. For anything that drops
/// or replaces the window from inside one of its own callbacks.
pub fn later(f: impl FnOnce(&Rc<App>) + 'static) {
    slint::Timer::single_shot(Duration::ZERO, move || {
        with(f);
    });
}

/// The whole app; returns the process exit code.
pub fn run(args: &[String]) -> Result<i32> {
    // Every context menu drawn by Slint (dark, like 1.2's) rather than as a
    // native Win32 menu through muda. Set before any thread starts.
    std::env::set_var("SLINT_NO_MUDA", "1");
    // The organizer's windows (made through shell::make) must never take
    // the foreground or get a taskbar button, from their very first show.
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|a| {
            if shell::making() {
                use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                a.with_active(false).with_skip_taskbar(true)
            } else {
                a
            }
        })
        .select()
        .map_err(|e| Error::msg(format!("Starting the UI: {e}")))?;
    let user_dir = instance::user_data_dir(args);
    let data_dir = instance::data_dir(user_dir.as_deref());
    let key = instance::key(&data_dir);
    let Some(_lock) = instance::acquire(&key)? else {
        instance::show_first(&key);
        return Ok(0);
    };
    std::fs::create_dir_all(&data_dir).map_err(|e| Error::msg(format!("Creating {}: {e}", data_dir.display())))?;
    let _com = win::Com::init();

    // First run of 1.3 in this folder: carry over 1.2.2's looks and its
    // start-with-Windows entry. The Run key is per user, not per folder, so a
    // test copy (--user-data-dir) never touches it.
    let first_run = !prefs::file(&data_dir).exists();
    let prefs = prefs::load_or_migrate(&data_dir);
    if first_run && user_dir.is_none() {
        let _ = autostart::migrate_from_electron();
    }

    let mut store = Store::new(&data_dir);
    let fresh = !store.apps_file().exists();
    let mut apps = store.load().unwrap_or_default();
    let mut first_icons = Vec::new();
    if fresh {
        // True first run: File Explorer and Recycle Bin, as 1.2.2 seeds them.
        (apps, first_icons) = launchpad_core::items::first_run(launchpad_core::store::new_id, launchpad_core::time::now_ms());
        if let Err(e) = store.save(&apps) {
            eprintln!("Failed to seed default apps: {e}");
        }
    }

    let msg = wtray::MsgWindow::new(&key, on_msg_event)?;
    let hotkey = Hotkey::new(&msg, HOTKEY_MAIN);
    let app = Rc::new(App {
        data_dir,
        store: RefCell::new(store),
        apps: RefCell::new(apps),
        prefs: RefCell::new(prefs),
        images: images::Images::new()?,
        window: RefCell::new(None),
        msg,
        tray_icon: RefCell::new(None),
        hotkey: RefCell::new(hotkey),
        return_focus: Cell::new(0),
        hotkey_follows_focus: RefCell::new(None),
        relaunch: Cell::new(false),
    });
    APP.with(|a| a.set(app.clone())).ok();

    app.add_tray_icon();
    icons::spawn(&app, first_icons, |_| {});
    // Before any panel docks: the space a killed run left reserved.
    organizer::panel::release_leftovers(&app);
    let s = app.settings();
    let accel = s.get("hotkey").and_then(|v| v.as_str()).filter(|a| !a.is_empty()).unwrap_or(DEFAULT_HOTKEY).to_string();
    if !app.hotkey.borrow_mut().set(&accel) {
        eprintln!("Could not register hotkey \"{accel}\" (already in use by another app).");
    }
    app.set_hotkey_follows_focus(launchpad_core::truthy(s.get("disableHotkeyInApps")));
    // Inside the running loop: winit only makes a window's HWND there, and the
    // panels and the main window need theirs straight after show().
    let _ = slint::invoke_from_event_loop(|| {
        with(|app| {
            organizer::configure(app);
            app.show(true);
        });
    });

    slint::run_event_loop_until_quit().map_err(|e| Error::msg(format!("The event loop failed: {e}")))?;
    APP.with(|a| {
        if let Some(app) = a.get() {
            media::discard();
            organizer::shutdown();
            organizer::panel::forget_docked(app);
            app.window.borrow_mut().take();
            app.tray_icon.borrow_mut().take();
            app.hotkey.borrow_mut().clear();
            app.hotkey_follows_focus.borrow_mut().take();
        }
    });
    if APP.with(|a| a.get().is_some_and(|app| app.relaunch.get())) {
        drop(_lock);
        if let Ok(exe) = std::env::current_exe() {
            if let Err(e) = win::launch::spawn(&exe.to_string_lossy(), args, "", true) {
                eprintln!("Couldn't start Launchpad again: {e}");
            }
        }
    }
    Ok(0)
}

fn on_msg_event(ev: wtray::Event) {
    with(|app| match ev {
        wtray::Event::Hotkey(HOTKEY_MAIN) | wtray::Event::TrayClick => app.toggle(),
        wtray::Event::ShowRequest => app.show(true),
        wtray::Event::TrayMenu { x, y } => app.tray_menu(x, y),
        wtray::Event::Hotkey(organizer::HOTKEY_GRID) => organizer::on_grid_hotkey(app),
        wtray::Event::DisplayChange => {
            let app = app.clone();
            // Sent while Windows is still rearranging: look once it's done.
            slint::Timer::single_shot(Duration::from_millis(200), move || organizer::displays_changed(&app));
        }
        wtray::Event::TaskbarCreated => {
            if let Some(t) = app.tray_icon.borrow().as_ref() {
                let _ = t.readd();
            }
        }
        _ => {}
    });
}

impl App {
    pub fn settings(&self) -> core_settings::Settings {
        core_settings::load(&self.data_dir)
    }

    /// Writes `patch` into settings.json; the merged settings back.
    pub fn save_settings(&self, patch: serde_json::Value) -> Result<core_settings::Settings> {
        let serde_json::Value::Object(patch) = patch else { return Err(Error::msg("Settings patch must be an object")) };
        core_settings::save(&self.data_dir, &patch)
    }

    /// Changes apps.json: `f` edits a copy, which is saved and only then
    /// becomes the app's state, so a failed save changes nothing. Every part
    /// is refreshed afterwards.
    pub fn mutate<R>(&self, f: impl FnOnce(&mut Vec<Entry>) -> Result<R>) -> Result<R> {
        let mut apps = self.apps.borrow().clone();
        let r = f(&mut apps)?;
        self.store.borrow().save(&apps)?;
        *self.apps.borrow_mut() = apps;
        self.refresh();
        Ok(r)
    }

    /// Re-pushes all state into the window, if there is one.
    pub fn refresh(&self) {
        let w = self.window.borrow().as_ref().map(|w| w.clone_strong());
        if let Some(w) = w {
            prefs::apply(self, &w);
            grid::refresh(self, &w);
            taskbar::refresh(self, &w);
            tray::refresh(self, &w);
            modals::refresh(self, &w);
            settings::refresh(self, &w);
            organizer::dialogs::refresh(self);
        }
        // Media follows Launchpad's theme and accent.
        if let Some(app) = with(Rc::clone) {
            media::theme_changed(&app);
        }
        // The panels follow the theme and the breadcrumb with or without the window.
        organizer::panel::push_content(self);
    }

    /// The main window's HWND, while it exists.
    pub fn hwnd(&self) -> Option<HWND> {
        self.window.borrow().as_ref().and_then(|w| hwnd_of(w.window()))
    }

    fn is_foreground(&self) -> bool {
        self.hwnd().is_some_and(|h| unsafe { GetForegroundWindow() } == h)
    }

    pub fn toggle(self: &Rc<Self>) {
        // While Media is open the hotkey belongs to it.
        if media::toggle(self) {
            return;
        }
        if self.window.borrow().is_some() && self.is_foreground() {
            self.hide();
        } else {
            self.show(true);
        }
    }

    /// Creates the window if needed and brings it forward.
    pub fn show(self: &Rc<Self>, focus: bool) {
        if focus && !self.is_foreground() {
            let fg = windowmgr::events::foreground();
            // Launchpad's own window in front: keep the app remembered last time.
            if fg.kind != windowmgr::events::Kind::Own {
                self.return_focus.set(if fg.kind == windowmgr::events::Kind::App { fg.hwnd } else { 0 });
            }
        }
        if self.window.borrow().is_none() {
            match self.create_window() {
                Ok(w) => *self.window.borrow_mut() = Some(w),
                Err(e) => {
                    eprintln!("Couldn't open the window: {e}");
                    return;
                }
            }
            self.refresh();
        }
        let w = self.window.borrow().as_ref().map(|w| w.clone_strong());
        if let Some(w) = w {
            let _ = w.show();
            shell::when_ready(&w, move |h| {
                // The panel carrying its top bar follows it between displays.
                shell::subclass(h, |_, msg, _, _| {
                    if msg == windows::Win32::UI::WindowsAndMessaging::WM_WINDOWPOSCHANGED {
                        later(organizer::panel::update_host);
                    }
                    None
                });
                later(organizer::panel::update_host);
                if focus {
                    unsafe {
                        let _ = SetForegroundWindow(h);
                    }
                }
            });
        }
    }

    /// Gives the focus back to where the user was, then hides and drops the
    /// window (hide first: dropping a shown window leaks its buffer).
    pub fn hide(self: &Rc<Self>) {
        let back = self.return_focus.replace(0);
        if back != 0 {
            let _ = windowmgr::focus(back);
        }
        later(|app| {
            let w = app.window.borrow_mut().take();
            if let Some(w) = w {
                let _ = w.hide();
            }
            app.images.clear();
            organizer::panel::update_host(app);
        });
    }

    /// Quits, then starts again with the same arguments (after Reset).
    pub fn relaunch(&self) {
        self.relaunch.set(true);
        self.quit();
    }

    /// A notification from the tray icon (1.2's `Notification`).
    pub fn notify(&self, title: &str, body: &str) {
        if let Some(t) = self.tray_icon.borrow().as_ref() {
            if let Err(e) = win::notify::show(t, title, body) {
                eprintln!("Notification: {e}");
            }
        }
    }

    pub fn quit(&self) {
        let _ = slint::quit_event_loop();
    }

    fn create_window(self: &Rc<Self>) -> Result<MainWindow> {
        let w = MainWindow::new().map_err(|e| Error::msg(format!("Creating the window: {e}")))?;
        prefs::bind(self, &w);
        grid::bind(self, &w);
        taskbar::bind(self, &w);
        tray::bind(self, &w);
        modals::bind(self, &w);
        settings::bind(self, &w);
        organizer::dialogs::bind(self, &w);
        if launchpad_core::truthy(self.settings().get("startFullscreen")) {
            w.window().set_fullscreen(true);
        }
        let weak = Rc::downgrade(self);
        w.window().on_close_requested(move || {
            if let Some(app) = weak.upgrade() {
                // "Minimize to tray on close", on unless turned off.
                if app.settings().get("minimizeToTrayOnClose") != Some(&serde_json::Value::Bool(false)) {
                    app.hide();
                } else {
                    app.quit();
                }
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
        Ok(w)
    }

    fn add_tray_icon(&self) {
        let Some(icon) = wtray::app_icon_path() else {
            eprintln!("No build\\icon.ico found; running without a tray icon.");
            return;
        };
        match wtray::Tray::new(&self.msg, &icon, "Launchpad") {
            Ok(t) => *self.tray_icon.borrow_mut() = Some(t),
            Err(e) => eprintln!("Couldn't add the tray icon: {e}"),
        }
    }

    fn tray_menu(self: &Rc<Self>, x: i32, y: i32) {
        use wtray::MenuItem::*;
        let on = autostart::is_enabled();
        let items = [Item(MENU_SHOW, "Show Launchpad"), Separator, Check(MENU_AUTOSTART, "Start with Windows", on), Separator, Item(MENU_QUIT, "Quit")];
        match wtray::popup_menu(self.msg.hwnd(), x, y, &items) {
            Some(MENU_SHOW) => self.show(true),
            Some(MENU_AUTOSTART) => {
                if let Err(e) = autostart::set_enabled(!on) {
                    eprintln!("Start with Windows: {e}");
                }
                self.refresh();
            }
            Some(MENU_QUIT) => self.quit(),
            _ => {}
        }
    }

    /// The "disable hotkey while another app is focused" setting: watches the
    /// foreground and unregisters the hotkey while an app (not the shell or
    /// Launchpad) has it.
    pub fn set_hotkey_follows_focus(&self, enable: bool) {
        if !enable {
            self.hotkey_follows_focus.borrow_mut().take();
            self.hotkey.borrow_mut().set_suspended(false);
            return;
        }
        if self.hotkey_follows_focus.borrow().is_some() {
            return;
        }
        let events = windowmgr::events::start(|ev| {
            if let windowmgr::events::WmEvent::Foreground(f) = ev {
                let suspend = f.kind == windowmgr::events::Kind::App;
                post(move |app| {
                    if app.hotkey_follows_focus.borrow().is_some() {
                        app.hotkey.borrow_mut().set_suspended(suspend);
                    }
                });
            }
        });
        match events {
            Ok(e) => {
                *self.hotkey_follows_focus.borrow_mut() = Some(e);
                let suspend = windowmgr::events::foreground().kind == windowmgr::events::Kind::App;
                self.hotkey.borrow_mut().set_suspended(suspend);
            }
            Err(e) => eprintln!("Couldn't watch the foreground window: {e}"),
        }
    }
}

/// A Slint window's HWND.
pub fn hwnd_of(w: &slint::Window) -> Option<HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match w.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}
