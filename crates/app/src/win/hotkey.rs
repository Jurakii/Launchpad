//! Global hotkeys (Electron's `globalShortcut`). Accelerators stay in
//! settings.json as Electron strings ("CommandOrControl+Space"); `parse`
//! turns one into `RegisterHotKey` arguments. `WM_HOTKEY` arrives at the
//! app's `tray::MsgWindow` as `Event::Hotkey(id)`.
use super::tray::{self, Event, MsgWindow};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// What `RegisterHotKey` takes. `mods` never includes `MOD_NOREPEAT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub mods: HOT_KEY_MODIFIERS,
    pub vk: u16,
}

// (name, vk); the first name for a vk is the one `format` writes. Electron's
// names plus the KeyboardEvent.key names acceleratorFromEvent passes through.
const NAMED: &[(&str, VIRTUAL_KEY)] = &[
    ("Space", VK_SPACE),
    ("Tab", VK_TAB),
    ("Backspace", VK_BACK),
    ("Delete", VK_DELETE),
    ("Insert", VK_INSERT),
    ("Enter", VK_RETURN),
    ("Return", VK_RETURN),
    ("Up", VK_UP),
    ("ArrowUp", VK_UP),
    ("Down", VK_DOWN),
    ("ArrowDown", VK_DOWN),
    ("Left", VK_LEFT),
    ("ArrowLeft", VK_LEFT),
    ("Right", VK_RIGHT),
    ("ArrowRight", VK_RIGHT),
    ("Home", VK_HOME),
    ("End", VK_END),
    ("PageUp", VK_PRIOR),
    ("PageDown", VK_NEXT),
    ("Escape", VK_ESCAPE),
    ("Esc", VK_ESCAPE),
    ("Capslock", VK_CAPITAL),
    ("Numlock", VK_NUMLOCK),
    ("Scrolllock", VK_SCROLL),
    ("PrintScreen", VK_SNAPSHOT),
    ("Pause", VK_PAUSE),
    ("ContextMenu", VK_APPS),
    ("Clear", VK_CLEAR),
    ("Help", VK_HELP),
    ("VolumeUp", VK_VOLUME_UP),
    ("AudioVolumeUp", VK_VOLUME_UP),
    ("VolumeDown", VK_VOLUME_DOWN),
    ("AudioVolumeDown", VK_VOLUME_DOWN),
    ("VolumeMute", VK_VOLUME_MUTE),
    ("AudioVolumeMute", VK_VOLUME_MUTE),
    ("MediaNextTrack", VK_MEDIA_NEXT_TRACK),
    ("MediaTrackNext", VK_MEDIA_NEXT_TRACK),
    ("MediaPreviousTrack", VK_MEDIA_PREV_TRACK),
    ("MediaTrackPrevious", VK_MEDIA_PREV_TRACK),
    ("MediaStop", VK_MEDIA_STOP),
    ("MediaPlayPause", VK_MEDIA_PLAY_PAUSE),
    ("BrowserBack", VK_BROWSER_BACK),
    ("BrowserForward", VK_BROWSER_FORWARD),
    ("BrowserRefresh", VK_BROWSER_REFRESH),
    ("BrowserStop", VK_BROWSER_STOP),
    ("BrowserSearch", VK_BROWSER_SEARCH),
    ("BrowserFavorites", VK_BROWSER_FAVORITES),
    ("BrowserHome", VK_BROWSER_HOME),
    ("LaunchMail", VK_LAUNCH_MAIL),
    ("LaunchMediaPlayer", VK_LAUNCH_MEDIA_SELECT),
    ("LaunchApplication1", VK_LAUNCH_APP1),
    ("LaunchApplication2", VK_LAUNCH_APP2),
    ("num0", VK_NUMPAD0),
    ("num1", VK_NUMPAD1),
    ("num2", VK_NUMPAD2),
    ("num3", VK_NUMPAD3),
    ("num4", VK_NUMPAD4),
    ("num5", VK_NUMPAD5),
    ("num6", VK_NUMPAD6),
    ("num7", VK_NUMPAD7),
    ("num8", VK_NUMPAD8),
    ("num9", VK_NUMPAD9),
    ("numdec", VK_DECIMAL),
    ("numadd", VK_ADD),
    ("numsub", VK_SUBTRACT),
    ("nummult", VK_MULTIPLY),
    ("numdiv", VK_DIVIDE),
    ("Plus", VK_OEM_PLUS), // shifted, see SHIFTED_NAMES
];

// Punctuation as (unshifted, shifted, vk): the US layout, as Electron maps it.
const PUNCT: &[(char, char, VIRTUAL_KEY)] = &[
    (';', ':', VK_OEM_1),
    ('=', '+', VK_OEM_PLUS),
    (',', '<', VK_OEM_COMMA),
    ('-', '_', VK_OEM_MINUS),
    ('.', '>', VK_OEM_PERIOD),
    ('/', '?', VK_OEM_2),
    ('`', '~', VK_OEM_3),
    ('[', '{', VK_OEM_4),
    ('\\', '|', VK_OEM_5),
    (']', '}', VK_OEM_6),
    ('\'', '"', VK_OEM_7),
];

// Shift+0 … Shift+9 on the US layout.
const SHIFTED_DIGITS: &str = ")!@#$%^&*(";

fn key(name: &str) -> Option<(VIRTUAL_KEY, bool)> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let u = c.to_ascii_uppercase();
        if u.is_ascii_uppercase() || u.is_ascii_digit() {
            return Some((VIRTUAL_KEY(u as u16), false));
        }
        if let Some(d) = SHIFTED_DIGITS.find(c) {
            return Some((VIRTUAL_KEY(b'0' as u16 + d as u16), true));
        }
        return PUNCT.iter().find_map(|&(n, s, vk)| (c == n).then_some((vk, false)).or((c == s).then_some((vk, true))));
    }
    if let Some(n) = name.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u16>().ok()) {
        return (1..=24).contains(&n).then_some((VIRTUAL_KEY(VK_F1.0 + n - 1), false));
    }
    let &(n, vk) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
    Some((vk, n == "Plus"))
}

fn modifier(name: &str) -> Option<HOT_KEY_MODIFIERS> {
    const MODS: &[(&str, HOT_KEY_MODIFIERS)] = &[
        ("CommandOrControl", MOD_CONTROL),
        ("CmdOrCtrl", MOD_CONTROL),
        ("Control", MOD_CONTROL),
        ("Ctrl", MOD_CONTROL),
        ("Alt", MOD_ALT),
        ("Option", MOD_ALT),
        ("Shift", MOD_SHIFT),
        ("Super", MOD_WIN),
        ("Meta", MOD_WIN),
        ("Command", MOD_WIN),
        ("Cmd", MOD_WIN),
    ];
    MODS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, m)| m)
}

/// An Electron accelerator, or None if it isn't one (or names a key this
/// can't map, e.g. "Dead" or a non-US letter).
pub fn parse(accel: &str) -> Option<Keys> {
    let accel = accel.trim();
    // "Shift++" is what acceleratorFromEvent makes for Shift and =.
    let (mods, key_name) = match accel.strip_suffix("++") {
        Some(rest) => (Some(rest), "+"),
        None if accel == "+" => (None, "+"),
        None => match accel.rsplit_once('+') {
            Some((m, k)) => (Some(m), k.trim()),
            None => (None, accel),
        },
    };
    let (vk, shifted) = key(key_name)?;
    let mut keys = Keys { mods: if shifted { MOD_SHIFT } else { HOT_KEY_MODIFIERS(0) }, vk: vk.0 };
    for m in mods.into_iter().flat_map(|m| m.split('+')) {
        keys.mods |= modifier(m.trim())?;
    }
    Some(keys)
}

/// The accelerator string for `keys`, in acceleratorFromEvent's form.
pub fn format(keys: Keys) -> String {
    let mut parts: Vec<String> = [(MOD_CONTROL, "CommandOrControl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Super")]
        .iter()
        .filter(|(m, _)| keys.mods.0 & m.0 != 0)
        .map(|(_, n)| n.to_string())
        .collect();
    let vk = keys.vk;
    let name = if (b'0' as u16..=b'9' as u16).contains(&vk) || (b'A' as u16..=b'Z' as u16).contains(&vk) {
        char::from(vk as u8).to_string()
    } else if (VK_F1.0..=VK_F24.0).contains(&vk) {
        format!("F{}", vk - VK_F1.0 + 1)
    } else if let Some(&(c, _, _)) = PUNCT.iter().find(|p| p.2 .0 == vk) {
        c.to_string()
    } else if let Some((n, _)) = NAMED.iter().find(|(_, v)| v.0 == vk) {
        n.to_string()
    } else {
        format!("0x{vk:02X}") // not reachable from parse
    };
    parts.push(name);
    parts.join("+")
}

/// Whether two accelerators are the same keys ("Ctrl+Space" and
/// "CommandOrControl+Space" are). For refusing one hotkey as the other.
pub fn same(a: &str, b: &str) -> bool {
    matches!((parse(a), parse(b)), (Some(x), Some(y)) if x == y)
}

/// One registered hotkey, with main.js's semantics: `set` swaps the old keys
/// for new ones and keeps the old ones if the new ones are taken; while
/// suspended nothing is registered and `set` only remembers.
pub struct Hotkey {
    hwnd: HWND,
    id: i32,
    accel: Option<(String, Keys)>,
    registered: bool,
    suspended: bool,
}

impl Hotkey {
    /// `id` comes back in `Event::Hotkey(id)`; one id per hotkey.
    pub fn new(window: &MsgWindow, id: i32) -> Hotkey {
        Hotkey { hwnd: window.hwnd(), id, accel: None, registered: false, suspended: false }
    }

    /// The current accelerator, as given to `set`.
    pub fn accel(&self) -> Option<&str> {
        self.accel.as_ref().map(|(a, _)| a.as_str())
    }

    /// Registers `accel` in place of the current one. False if it isn't an
    /// accelerator or another app (or another of ours) has it; the old one
    /// then stays registered.
    pub fn set(&mut self, accel: &str) -> bool {
        let Some(keys) = parse(accel) else { return false };
        if self.suspended {
            self.accel = Some((accel.to_string(), keys));
            return true;
        }
        if self.registered && self.accel.as_ref().is_some_and(|(_, k)| *k == keys) {
            self.accel = Some((accel.to_string(), keys));
            return true;
        }
        self.unregister();
        if self.register(keys) {
            self.accel = Some((accel.to_string(), keys));
            return true;
        }
        if let Some((_, old)) = self.accel {
            self.register(old);
        }
        false
    }

    /// Unregisters while suspended ("disable hotkey while another app is
    /// focused"). Resuming returns false if the keys were taken meanwhile.
    pub fn set_suspended(&mut self, suspend: bool) -> bool {
        if suspend == self.suspended {
            return true;
        }
        self.suspended = suspend;
        if suspend {
            self.unregister();
            return true;
        }
        match self.accel {
            Some((_, keys)) => self.register(keys),
            None => true,
        }
    }

    /// Unregisters and forgets the accelerator.
    pub fn clear(&mut self) {
        self.unregister();
        self.accel = None;
    }

    fn register(&mut self, keys: Keys) -> bool {
        // Fails with ERROR_HOTKEY_ALREADY_REGISTERED when anyone has the keys.
        self.registered = unsafe { RegisterHotKey(Some(self.hwnd), self.id, keys.mods | MOD_NOREPEAT, keys.vk as u32) }.is_ok();
        self.registered
    }

    fn unregister(&mut self) {
        if std::mem::take(&mut self.registered) {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), self.id);
            }
        }
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        self.unregister();
    }
}

pub fn probe(args: &[String]) -> Result<Value> {
    let action = args.first().map(String::as_str);
    let accel = args.get(1).map(String::as_str);
    let parsed = |a: &str| parse(a).ok_or_else(|| Error::msg(format!("\"{a}\" isn't an accelerator Launchpad understands.")));
    match (action, accel) {
        (Some("parse"), Some(a)) => {
            let k = parsed(a)?;
            Ok(json!({ "accelerator": a, "mods": k.mods.0, "vk": k.vk, "formatted": format(k) }))
        }
        (Some("try"), Some(a)) => {
            parsed(a)?;
            let secs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10.0);
            let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
            let f = fired.clone();
            let window = MsgWindow::new("LaunchpadProbe-hotkey", move |e| {
                if let Event::Hotkey(id) = e {
                    f.set(f.get() + 1);
                    eprintln!("fired (id {id})");
                }
            })?;
            let mut hk = Hotkey::new(&window, 1);
            if !hk.set(a) {
                return Ok(json!({ "accelerator": a, "registered": false, "taken": true }));
            }
            // Suspend and resume once, as "disable while another app is focused" does.
            let resumed = hk.set_suspended(true) && hk.set_suspended(false);
            eprintln!("{a} registered for {secs} s; press it.");
            tray::pump_for(secs);
            drop(hk);
            Ok(json!({ "accelerator": a, "registered": true, "resumedAfterSuspend": resumed, "fired": fired.get() }))
        }
        // main.js's swap: register `a`, then try `b` in its place.
        (Some("swap"), Some(a)) => {
            let b = args.get(2).ok_or_else(|| Error::msg("swap needs two accelerators."))?;
            if same(a, b) {
                return Err(Error::msg("Those are the same keys."));
            }
            let window = MsgWindow::new("LaunchpadProbe-hotkey", |_| {})?;
            let mut hk = Hotkey::new(&window, 1);
            let first = hk.set(a);
            let second = hk.set(b);
            let now = hk.accel().map(String::from);
            hk.clear();
            Ok(json!({ "first": a, "firstRegistered": first, "second": b, "secondRegistered": second, "registeredAfter": now }))
        }
        _ => Err(Error::msg("Usage: hotkey parse <accel> | try <accel> [seconds] | swap <accel> <accel>")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(mods: HOT_KEY_MODIFIERS, vk: VIRTUAL_KEY) -> Option<Keys> {
        Some(Keys { mods, vk: vk.0 })
    }

    #[test]
    fn parses_the_stored_defaults() {
        assert_eq!(parse("CommandOrControl+Space"), k(MOD_CONTROL, VK_SPACE));
        assert_eq!(parse("CommandOrControl+Alt+G"), k(MOD_CONTROL | MOD_ALT, VIRTUAL_KEY(b'G' as u16)));
        assert_eq!(parse("cmdorctrl + shift + f12"), k(MOD_CONTROL | MOD_SHIFT, VK_F12));
        assert_eq!(parse("Super+Up"), k(MOD_WIN, VK_UP));
        assert_eq!(parse("F13"), k(HOT_KEY_MODIFIERS(0), VK_F13));
    }

    #[test]
    fn shifted_characters_add_shift() {
        assert_eq!(parse("CommandOrControl+Shift++"), k(MOD_CONTROL | MOD_SHIFT, VK_OEM_PLUS));
        assert_eq!(parse("Ctrl+Plus"), k(MOD_CONTROL | MOD_SHIFT, VK_OEM_PLUS));
        assert_eq!(parse("Alt+!"), k(MOD_ALT | MOD_SHIFT, VIRTUAL_KEY(b'1' as u16)));
        assert_eq!(parse("Ctrl+?"), k(MOD_CONTROL | MOD_SHIFT, VK_OEM_2));
        assert_eq!(parse("+"), k(MOD_SHIFT, VK_OEM_PLUS));
    }

    #[test]
    fn refuses_what_it_cannot_map() {
        for bad in ["", "Ctrl+", "Ctrl+Dead", "Ctrl+Unidentified", "Ctrl+Alt+AltGraph", "Ctrl+é", "Hyper+A", "F25", "F0", "Ctrl+Shift"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_name_round_trips() {
        let mut names: Vec<String> = NAMED.iter().map(|(n, _)| n.to_string()).collect();
        names.extend((1..=24).map(|n| format!("F{n}")));
        names.extend(('0'..='9').chain('A'..='Z').map(String::from));
        names.extend(PUNCT.iter().flat_map(|&(a, b, _)| [a.to_string(), b.to_string()]));
        names.extend(SHIFTED_DIGITS.chars().map(String::from));
        for name in names {
            for mods in ["", "CommandOrControl+", "CommandOrControl+Alt+", "Alt+Shift+", "CommandOrControl+Alt+Shift+Super+"] {
                let accel = format!("{mods}{name}");
                let keys = parse(&accel).unwrap_or_else(|| panic!("{accel} doesn't parse"));
                let again = format(keys);
                assert_eq!(parse(&again), Some(keys), "{accel} -> {again}");
            }
        }
    }

    #[test]
    fn canonical_names_format_back_unchanged() {
        for accel in ["CommandOrControl+Space", "CommandOrControl+Alt+G", "Alt+Shift+F24", "CommandOrControl+;", "Shift+[", "Super+num5", "CommandOrControl+Alt+Shift+Super+PageDown", "VolumeUp", "CommandOrControl+7"] {
            assert_eq!(format(parse(accel).unwrap()), accel);
        }
        // acceleratorFromEvent's arrow names come back as Electron's.
        assert_eq!(format(parse("CommandOrControl+ArrowLeft").unwrap()), "CommandOrControl+Left");
    }

    #[test]
    fn same_compares_keys_not_spelling() {
        assert!(same("Ctrl+Space", "CommandOrControl+Space"));
        assert!(same("CmdOrCtrl+Alt+g", "Control+Option+G"));
        assert!(!same("Ctrl+Space", "Alt+Space"));
        assert!(!same("Nonsense", "Nonsense"));
    }
}
