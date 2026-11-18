//! The visual prefs (ui.json: what 1.2.2 kept in Chromium's localStorage,
//! under the same key names) and pushing them into the `Theme` global.
use super::{App, MainWindow, Theme};
use launchpad_core::ui_prefs::{self, UiPrefs};
use slint::{Color, ComponentHandle};
use std::path::{Path, PathBuf};

pub type Prefs = UiPrefs;

pub fn file(dir: &Path) -> PathBuf {
    dir.join(ui_prefs::UI_FILE)
}

/// ui.json, or on the first run what 1.2.2 left in Chromium's localStorage.
/// Never fails: anything wrong means default looks.
pub fn load_or_migrate(dir: &Path) -> Prefs {
    if let Err(e) = ui_prefs::migrate(dir) {
        eprintln!("Carrying over 1.2.2's looks: {e}");
    }
    UiPrefs::load(dir)
}

impl App {
    /// A pref's value, as localStorage held it.
    pub fn pref(&self, key: &str) -> Option<String> {
        self.prefs.borrow().get(key).map(str::to_string)
    }

    /// Sets (Some) or removes (None) a pref, saves ui.json and refreshes the window.
    pub fn set_pref(&self, key: &str, value: Option<&str>) {
        {
            let mut p = self.prefs.borrow_mut();
            match value {
                Some(v) => p.set(key, v),
                None => p.remove(key),
            }
            if let Err(e) = p.save(&self.data_dir) {
                eprintln!("Saving ui.json: {e}");
            }
        }
        // Everything: some parts read prefs too (the tray's order mirrors with the pinned side).
        self.refresh();
    }
}

pub fn bind(_app: &App, _w: &MainWindow) {}

pub fn apply(app: &App, w: &MainWindow) {
    apply_theme(app, &w.global::<Theme>());
    // A file name in the data folder (core::ui_prefs).
    let wp = app.pref("launchpad-wallpaper").and_then(|name| app.images.wallpaper(&app.data_dir.join(name)));
    w.set_wallpaper(wp.unwrap_or_default());
}

/// The `Theme` global of any of Launchpad's windows (each window has its
/// own copy of a global): the main window, the organizer's panels, menus.
pub fn apply_theme(app: &App, t: &Theme<'_>) {
    t.set_light(app.pref("launchpad-theme").as_deref() == Some("light"));
    t.set_accent_override(app.pref("launchpad-accent").and_then(|c| parse_hex(&c)).unwrap_or(Color::from_argb_u8(0, 0, 0, 0)));
    t.set_taskbar_edge(app.pref("launchpad-taskbar-edge").filter(|e| ["top", "left", "right"].contains(&e.as_str())).unwrap_or("bottom".into()).into());
    t.set_taskbar_style(app.pref("launchpad-taskbar-style").unwrap_or("solid".into()).into());
    t.set_topbar_style(app.pref("launchpad-topbar-style").unwrap_or("solid".into()).into());
    t.set_hide_add_buttons(app.pref("launchpad-hide-add-buttons").as_deref() == Some("1"));
    t.set_hide_app_names(app.pref("launchpad-hide-app-names").as_deref() == Some("1"));
}

/// "#rgb" / "#rrggbb", as an <input type=color> or the accent pref holds it.
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?;
    let n = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some(Color::from_rgb_u8((n >> 16) as u8, (n >> 8) as u8, n as u8)),
        3 => Some(Color::from_rgb_u8(((n >> 8) & 15) as u8 * 17, ((n >> 4) & 15) as u8 * 17, (n & 15) as u8 * 17)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#5b8cff"), Some(Color::from_rgb_u8(0x5b, 0x8c, 0xff)));
        assert_eq!(parse_hex("#fff"), Some(Color::from_rgb_u8(255, 255, 255)));
        assert_eq!(parse_hex("5b8cff"), None);
        assert_eq!(parse_hex("#12345"), None);
    }
}
