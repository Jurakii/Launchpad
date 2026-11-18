//! Media's Settings: players and emulators (profiles.json), colours (ui.json's
//! launchpad-media-colors), the controller (settings.json's `controller`) and
//! the library defaults (ui.json's launchpad-media-library-defaults).
use super::{active_mapping, active_pad, apply_theme, bump, button_name, mapping_for, md, render_prompts, set_focus, show_screen, st, toast, App, Capture, Media, LIBRARY_DEFAULTS_KEY};
use crate::ui::{MColor, MItem, MMap};
use crate::win::dialogs;
use launchpad_core::media::controller;
use launchpad_core::media::pad::assign_button;
use launchpad_core::media::players::{self, Profile, PRESETS};
use launchpad_core::media::theme::{self, STORAGE_KEY, TOKENS};
use serde_json::{json, Map, Value};
use slint::{ModelRc, Timer, TimerMode, VecModel};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

const SECTIONS: [&str; 4] = ["profiles", "colors", "controller", "library"];
const ACTIONS: [(&str, &str); 13] = [
    ("select", "Select or launch"),
    ("back", "Back"),
    ("customize", "Customize art"),
    ("quick", "Quick actions"),
    ("tabPrev", "Previous tab"),
    ("tabNext", "Next tab"),
    ("pageUp", "Page up"),
    ("pageDown", "Page down"),
    ("menu", "Menu (Settings)"),
    ("up", "Move up"),
    ("down", "Move down"),
    ("left", "Move left"),
    ("right", "Move right"),
];
const KINDS: [(&str, &str); 2] = [("player", "Player (videos)"), ("emulator", "Emulator (games)")];

fn action_label(a: &str) -> &'static str {
    ACTIONS.iter().find(|(k, _)| *k == a).map_or("", |(_, l)| l)
}

/// The profile being edited.
pub struct Editing {
    id: String,
    is_new: bool,
    kind: &'static str,
    default: bool,
}

pub fn open(app: &Rc<App>, section: Option<&'static str>) {
    let section = section.or_else(|| md(|m| m.section)).unwrap_or("profiles");
    show_section(app, section);
    show_screen(app, "settings");
}

/// LB / RB on Settings, wrapping at the ends like Home's tabs.
pub fn switch_section(app: &Rc<App>, step: i32) {
    let i = md(|m| SECTIONS.iter().position(|s| *s == m.section).unwrap_or(0) as i32).unwrap_or(0);
    show_section(app, SECTIONS[(i + step).rem_euclid(4) as usize]);
}

fn show_section(app: &Rc<App>, section: &'static str) {
    if md(|m| m.capture.is_some()).unwrap_or(false) {
        cancel_capture(app);
    }
    md(|m| {
        m.section = section;
        st(m).set_section(section.into());
    });
    match section {
        "profiles" => {
            load_profiles(app);
            md(|m| {
                m.editing = None;
                st(m).set_pf_open(false);
            });
            render_profiles();
        }
        "controller" => {
            let c = controller::load(&app.data_dir);
            md(|m| {
                m.controller = c;
                st(m).set_capture_status("".into());
            });
            render_controller();
        }
        "colors" => render_colors(app),
        _ => render_library(app),
    }
    if md(|m| m.screen == "settings").unwrap_or(false) {
        set_focus(app, &format!("sec:{section}"));
    }
}

pub fn activate(app: &Rc<App>, kind: &str, rest: &str) {
    match (kind, rest) {
        ("sec", s) => {
            if let Some(s) = SECTIONS.iter().find(|x| **x == s) {
                show_section(app, s);
            }
        }
        ("profile", id) => {
            if let Some(p) = md(|m| m.profiles.profiles.iter().find(|p| p.id == id).cloned()).flatten() {
                edit(app, p, false);
            }
        }
        ("preset", id) => {
            let exe = md(|m| m.detected.iter().find(|d| d.id == id).map(|d| d.exe.clone())).flatten().unwrap_or_default();
            if let Ok(p) = players::from_preset(id, &exe) {
                edit(app, p, true);
            }
        }
        ("pf", "new") => edit(app, Profile { id: String::new(), name: "New profile".into(), kind: "emulator".into(), exe: String::new(), args: vec!["{file}".into()], platforms: Vec::new() }, true),
        ("pf", "kind") => {
            let cur = md(|m| m.editing.as_ref().map(|e| e.kind)).flatten().unwrap_or("emulator");
            pick(app, "pf", "kind", usize::from(cur == "player"));
        }
        ("pf", "browse") => {
            let filters: [dialogs::Filter; 1] = [("Programs", &["exe"])];
            if let Ok(Some(p)) = dialogs::open_file(md(|m| m.hwnd).flatten(), &filters, None) {
                md(|m| st(m).set_pf_exe(p.to_string_lossy().into_owned().into()));
            }
        }
        ("pf", "default") => {
            md(|m| {
                let on = m.editing.as_mut().map(|e| {
                    e.default = !e.default;
                    e.default
                });
                if let Some(on) = on {
                    st(m).set_pf_default(on);
                }
            });
        }
        ("pf", "save") => save_profile(app),
        ("pf", "delete") => delete_profile(app),
        ("reset", token) => {
            let mut o = color_overrides(app);
            o.remove(token);
            save_colors(app, o);
            set_focus(app, &format!("color:{token}"));
        }
        ("colors", "reset") => {
            save_colors(app, Map::new());
            set_focus(app, "sec:colors");
            toast("Colours are back to the defaults");
        }
        ("map", "reset") => {
            let Some(pad) = md(|m| active_pad(m)).flatten() else { return };
            md(|m| m.controller.pads.retain(|(id, _)| *id != pad));
            save_controller(app);
            md(|m| st(m).set_capture_status("Back to the standard layout.".into()));
            render_controller();
        }
        ("map", action) => start_capture(app, action),
        ("dz", step) => {
            md(|m| {
                let d = m.controller.dead_zone + if step == "plus" { 0.05 } else { -0.05 };
                m.controller.dead_zone = ((d * 20.0).round() / 20.0).clamp(0.1, 0.9);
            });
            save_controller(app);
            render_controller();
        }
        ("lib", "type") => {
            let (l, _) = library_defaults(app);
            pick(app, "lib", "type", usize::from(l == "games"));
        }
        ("lib", "recursive") => {
            let (l, r) = library_defaults(app);
            save_library_defaults(app, l, !r);
        }
        _ => {}
    }
}

/// A dropdown in Settings.
pub fn pick(app: &Rc<App>, kind: &str, rest: &str, i: usize) {
    match (kind, rest) {
        ("pf", "kind") => {
            md(|m| {
                if let Some(e) = m.editing.as_mut() {
                    e.kind = KINDS[i.min(1)].0;
                    st(m).set_pf_kind(KINDS[i.min(1)].1.into());
                }
            });
        }
        ("lib", "type") => {
            let (_, r) = library_defaults(app);
            save_library_defaults(app, if i == 1 { "videos" } else { "games" }, r);
        }
        _ => {}
    }
}

/// Enter in one of Settings' fields.
pub fn accepted(app: &Rc<App>, kind: &str, rest: &str) {
    match kind {
        "pf" => save_profile(app),
        "color" => {
            let key = format!("color:{rest}");
            let Some(text) = md(|m| m.edits.get(&key).cloned()).flatten() else { return };
            let t = text.trim().to_ascii_lowercase();
            let hex = if t.starts_with('#') { t } else { format!("#{t}") };
            if hex.len() != 7 || !hex[1..].chars().all(|c| c.is_ascii_hexdigit()) {
                return toast("Type a colour as #rrggbb, for example #5b8cff");
            }
            let mut o = color_overrides(app);
            o.insert(rest.to_string(), hex.into());
            save_colors(app, o);
            set_focus(app, &key);
        }
        _ => {}
    }
}

// ---------- Players and emulators ----------

fn load_profiles(app: &App) {
    let data = match players::load(&app.data_dir) {
        Ok(d) => d,
        Err(e) => {
            toast(&e.to_string());
            Default::default()
        }
    };
    let detected = players::detect_installed(|p| Path::new(p).exists());
    md(|m| {
        m.profiles = data;
        m.detected = detected;
    });
}

fn render_profiles() {
    md(|m| {
        let editing = m.editing.as_ref().map(|e| e.id.clone());
        let list: Vec<MItem> = m
            .profiles
            .profiles
            .iter()
            .map(|p| {
                let is_default = m.profiles.defaults.values().any(|v| v.as_str() == Some(&p.id));
                let kind = if p.kind == "player" { "Player" } else { "Emulator" };
                let sub: Vec<String> = [kind.to_string(), p.platforms.join(", "), if is_default { "default".into() } else { String::new() }]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect();
                MItem { id: p.id.as_str().into(), title: p.name.as_str().into(), sub: sub.join(" · ").into(), selected: editing.as_deref() == Some(&p.id) }
            })
            .collect();
        let presets: Vec<MItem> = PRESETS
            .iter()
            .map(|p| {
                let found = m.detected.iter().any(|d| d.id == p.id);
                MItem { id: p.id.into(), title: if found { format!("{} (installed)", p.name) } else { p.name.to_string() }.into(), sub: "".into(), selected: false }
            })
            .collect();
        let s = st(m);
        s.set_profiles(ModelRc::new(VecModel::from(list)));
        s.set_presets(ModelRc::new(VecModel::from(presets)));
        bump(m);
    });
}

// renderer media.js slug(): lower-case, dashes, at most 48 characters.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t: String = out.trim_matches('-').chars().take(48).collect();
    if t.is_empty() { "profile".into() } else { t }
}

fn unique_id(m: &Media, base: &str) -> String {
    let taken = |id: &str| m.profiles.profiles.iter().any(|p| p.id == id);
    if !taken(base) {
        return base.into();
    }
    (2..).map(|i| format!("{base}-{i}")).find(|id| !taken(id)).unwrap_or_default()
}

fn edit(app: &Rc<App>, p: Profile, is_new: bool) {
    md(|m| {
        let id = if is_new { unique_id(m, &slug(if p.id.is_empty() { &p.name } else { &p.id })) } else { p.id.clone() };
        // New profiles become the default where none is set yet.
        let default = if is_new {
            p.platforms.iter().any(|pl| !m.profiles.defaults.contains_key(pl))
        } else {
            m.profiles.defaults.values().any(|v| v.as_str() == Some(&p.id))
        };
        let kind = if p.kind == "player" { "player" } else { "emulator" };
        let s = st(m);
        s.set_pf_name(p.name.as_str().into());
        s.set_pf_kind(KINDS.iter().find(|(k, _)| *k == kind).map_or("", |(_, l)| l).into());
        s.set_pf_exe(p.exe.as_str().into());
        s.set_pf_args(p.args.join("\n").into());
        s.set_pf_platforms(p.platforms.join(", ").into());
        s.set_pf_default(default);
        s.set_pf_new(is_new);
        s.set_pf_error("".into());
        s.set_pf_open(true);
        m.editing = Some(Editing { id, is_new, kind, default });
    });
    render_profiles();
    set_focus(app, "pf:name");
}

fn save_profiles(app: &App, data: Value) -> bool {
    // Any invalid profile refuses the save, with its problems, rather than being dropped silently.
    for p in data["profiles"].as_array().into_iter().flatten() {
        let errors = players::validate_profile(p);
        if !errors.is_empty() {
            md(|m| st(m).set_pf_error(errors.join(". ").into()));
            return false;
        }
    }
    match players::save(&app.data_dir, &data) {
        Ok(_) => {
            load_profiles(app);
            true
        }
        Err(e) => {
            md(|m| st(m).set_pf_error(e.to_string().into()));
            false
        }
    }
}

fn save_profile(app: &Rc<App>) {
    let Some((profile, data)) = md(|m| {
        let e = m.editing.as_ref()?;
        let s = st(m);
        let profile = Profile {
            id: e.id.clone(),
            name: s.get_pf_name().trim().to_string(),
            kind: e.kind.to_string(),
            exe: s.get_pf_exe().trim().to_string(),
            args: s.get_pf_args().split('\n').map(str::trim).filter(|a| !a.is_empty()).map(String::from).collect(),
            platforms: s.get_pf_platforms().split(',').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect(),
        };
        let mut defaults = m.profiles.defaults.clone();
        defaults.retain(|key, id| !(id.as_str() == Some(&profile.id) && (!e.default || !profile.platforms.contains(key))));
        if e.default {
            for p in &profile.platforms {
                defaults.insert(p.clone(), profile.id.as_str().into());
            }
        }
        let mut list: Vec<Profile> = m.profiles.profiles.iter().filter(|p| p.id != profile.id).cloned().collect();
        list.push(profile.clone());
        Some((profile, json!({ "profiles": list, "defaults": defaults })))
    })
    .flatten() else {
        return;
    };
    if save_profiles(app, data) {
        toast(&format!("Saved {}", profile.name));
        edit(app, profile, false);
    }
}

fn delete_profile(app: &Rc<App>) {
    let Some(data) = md(|m| {
        let id = m.editing.as_ref().filter(|e| !e.is_new)?.id.clone();
        let mut defaults = m.profiles.defaults.clone();
        defaults.retain(|_, v| v.as_str() != Some(&id));
        let list: Vec<&Profile> = m.profiles.profiles.iter().filter(|p| p.id != id).collect();
        Some(json!({ "profiles": list, "defaults": defaults }))
    })
    .flatten() else {
        return;
    };
    if save_profiles(app, data) {
        toast("Deleted");
        md(|m| {
            m.editing = None;
            st(m).set_pf_open(false);
        });
        render_profiles();
        let first = md(|m| m.profiles.profiles.first().map(|p| format!("profile:{}", p.id))).flatten().unwrap_or_else(|| format!("preset:{}", PRESETS[0].id));
        set_focus(app, &first);
    }
}

// ---------- Colours ----------

/// The colour overrides in ui.json, cleaned (core::media::theme::clean).
pub fn color_overrides(app: &App) -> Map<String, Value> {
    let raw = app.pref(STORAGE_KEY).and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
    theme::clean(&raw)
}

fn save_colors(app: &Rc<App>, overrides: Map<String, Value>) {
    let c = theme::clean(&Value::Object(overrides));
    let text = (!c.is_empty()).then(|| Value::Object(c).to_string());
    app.set_pref(STORAGE_KEY, text.as_deref());
    apply_theme(app);
    md(|m| m.edits.retain(|k, _| !k.starts_with("color:")));
    render_colors(app);
}

fn render_colors(app: &App) {
    let overrides = color_overrides(app);
    let values = theme::effective(&overrides, &app.pref("launchpad-theme").unwrap_or_default(), app.pref("launchpad-accent").as_deref());
    let low = theme::low_contrast(&values);
    md(|m| {
        let rows: Vec<MColor> = TOKENS
            .iter()
            .map(|t| {
                let v = values.iter().find(|(k, _)| *k == t.token).map_or(t.dark, |(_, v)| v.as_str()).to_string();
                MColor {
                    token: t.token.into(),
                    label: t.label.into(),
                    color: crate::ui::prefs::parse_hex(&v).unwrap_or_default(),
                    value: v.into(),
                    changed: overrides.contains_key(t.token),
                }
            })
            .collect();
        let s = st(m);
        s.set_colors(ModelRc::new(VecModel::from(rows)));
        // A warning, not a block: low contrast can be a deliberate choice.
        s.set_contrast_warn(if low.is_empty() {
            "".into()
        } else {
            let list: Vec<String> = low.iter().map(|p| format!("{} ({:.1}:1)", p.label, p.ratio)).collect();
            format!("Hard to read: {}. 4.5:1 or more reads well.", list.join(", ")).into()
        });
        s.set_colors_changed(!overrides.is_empty());
        bump(m);
    });
}

// ---------- Controller ----------

fn render_controller() {
    md(|m| {
        let pad = active_pad(m);
        let mapping = active_mapping(m);
        let capturing = m.capture.as_ref().map(|c| c.action.clone());
        let rows: Vec<MMap> = ACTIONS
            .iter()
            .map(|(action, label)| {
                let b = mapping.iter().find(|(_, a)| a == action).map(|(b, _)| *b);
                MMap {
                    action: (*action).into(),
                    label: (*label).into(),
                    capturing: capturing.as_deref() == Some(action),
                    button: if capturing.as_deref() == Some(action) {
                        "Press a button…".into()
                    } else {
                        b.map_or_else(|| "None".to_string(), button_name).into()
                    },
                }
            })
            .collect();
        let s = st(m);
        s.set_pad_name(match &pad {
            Some(id) => format!("Setting up: {id}").into(),
            None => "No controller connected. Connect one and press any button on it.".into(),
        });
        s.set_maps(ModelRc::new(VecModel::from(rows)));
        s.set_maps_enabled(pad.is_some());
        s.set_dead_zone(format!("{}%", (m.controller.dead_zone * 100.0).round()).into());
        s.set_map_reset_enabled(pad.as_ref().is_some_and(|p| m.controller.mapping_for(p).is_some()));
        bump(m);
    });
}

fn save_controller(app: &App) {
    let Some(json) = md(|m| m.controller.to_json()) else { return };
    match controller::save(&app.data_dir, &json) {
        Ok(c) => {
            md(|m| {
                m.reader.configure(Some(mapping_for(&c)), Some(c.dead_zone));
                m.controller = c;
            });
            render_prompts();
        }
        Err(e) => toast(&e.to_string()),
    }
}

fn start_capture(app: &Rc<App>, action: &str) {
    if md(|m| active_pad(m)).flatten().is_none() || !ACTIONS.iter().any(|(a, _)| *a == action) {
        return;
    }
    // Every pad button is captured, so cancelling is Esc or just waiting.
    let timeout = Timer::default();
    timeout.start(TimerMode::SingleShot, Duration::from_secs(8), || {
        crate::ui::with(cancel_capture);
    });
    md(|m| {
        m.capture = Some(Capture { action: action.to_string(), held: HashMap::new(), _timeout: timeout });
        st(m).set_capture_status(format!("Press the button for \"{}\". Wait 8 seconds or press Esc to cancel.", action_label(action)).into());
    });
    render_controller();
    let _ = app;
}

pub fn cancel_capture(app: &Rc<App>) {
    let action = md(|m| {
        st(m).set_capture_status("".into());
        m.capture.take().map(|c| c.action)
    })
    .flatten();
    render_controller();
    if let Some(a) = action {
        set_focus(app, &format!("map:{a}"));
    }
}

/// The button pressed during a capture, on the pad with that id.
pub fn captured(app: &Rc<App>, button: u32, pad: &str) {
    let Some(action) = md(|m| m.capture.take().map(|c| c.action)).flatten() else { return };
    md(|m| {
        let current = m.controller.mapping_for(pad).cloned().unwrap_or_else(launchpad_core::media::pad::default_mapping);
        let out = assign_button(&current, &action, button);
        let (b, a) = (button_name(button), action_label(&action));
        let status = match (&out.swapped, &out.unmapped) {
            (Some(s), _) => format!("{b} is now {a}. {} moved to its old button.", action_label(s)),
            (_, Some(u)) => format!("{b} is now {a}. {} has no button now.", action_label(u)),
            _ => format!("{b} is now {a}."),
        };
        match m.controller.pads.iter_mut().find(|(id, _)| id == pad) {
            Some((_, map)) => *map = out.mapping,
            None => m.controller.pads.push((pad.to_string(), out.mapping)),
        }
        st(m).set_capture_status(status.into());
    });
    save_controller(app);
    render_controller();
    set_focus(app, &format!("map:{action}"));
}

/// A pad came or went.
pub fn pads_changed(_app: &Rc<App>) {
    if md(|m| m.screen == "settings" && m.section == "controller").unwrap_or(false) {
        render_controller();
    }
}

// ---------- Library defaults ----------

/// What Add → Folder starts with: (games|videos, include subfolders).
pub fn library_defaults(app: &App) -> (&'static str, bool) {
    let saved = app.pref(LIBRARY_DEFAULTS_KEY).and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
    let library = if saved.get("library").and_then(Value::as_str) == Some("videos") { "videos" } else { "games" };
    (library, saved.get("recursive") != Some(&Value::Bool(false)))
}

fn save_library_defaults(app: &Rc<App>, library: &str, recursive: bool) {
    app.set_pref(LIBRARY_DEFAULTS_KEY, Some(&json!({ "library": library, "recursive": recursive }).to_string()));
    render_library(app);
}

fn render_library(app: &App) {
    let (l, r) = library_defaults(app);
    md(|m| {
        let s = st(m);
        s.set_lib_type(if l == "videos" { "Videos" } else { "Games" }.into());
        s.set_lib_recursive(r);
        bump(m);
    });
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn slugs_are_lower_case_with_dashes() {
        assert_eq!(slug("RetroArch (Snes9x)"), "retroarch-snes9x");
        assert_eq!(slug("  !!"), "profile");
        assert_eq!(slug(&"a".repeat(60)).len(), 48);
        assert_eq!(slug("Dolphin 5.0"), "dolphin-5-0");
    }
}
