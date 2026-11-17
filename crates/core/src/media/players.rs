//! Media's player and emulator profiles (profiles.json): which program
//! opens a video or ROM, with which arguments. Args are an array of strings,
//! never one command line, so a file name is passed as exactly one argument
//! with no quoting or splitting - see build_launch().
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::library::Entry;
use super::{ends_with_ignore_case, js_trim, truthy, win_path};
use crate::safe_file::{corrupt_copy_path, write_file_atomic};
use crate::{Error, Result};

pub fn profiles_file(dir: &Path) -> PathBuf {
    dir.join("profiles.json")
}

pub const KINDS: [&str; 2] = ["player", "emulator"];
// Platform ids are short slugs (snes, psx, n64); 'videos' is the key the
// video library uses for its default player.
pub const VIDEOS: &str = "videos";
const ID_MAX: usize = 64;
const PLATFORM_MAX: usize = 32;

// /^[a-z0-9][a-z0-9-]{0,max-1}$/
fn is_slug(s: &str, max: usize) -> bool {
    let b = s.as_bytes();
    let alnum = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    !b.is_empty() && b.len() <= max && alnum(&b[0]) && b[1..].iter().all(|c| alnum(c) || *c == b'-')
}

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub args: &'static [&'static str],
    pub platforms: &'static [&'static str],
    /// The usual install locations.
    pub candidates: &'static [&'static str],
}

// Both RetroArch presets look in the same places.
const RETROARCH_CANDIDATES: &[&str] = &["C:\\RetroArch-Win64\\retroarch.exe", "C:\\Program Files\\RetroArch\\retroarch.exe"];

/// Starting points for the profile editor. The program is filled in by the
/// user or by detect_installed().
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "mpv",
        name: "mpv",
        kind: "player",
        args: &["--fs", "{file}"],
        platforms: &[VIDEOS],
        candidates: &["C:\\Program Files\\mpv\\mpv.exe", "C:\\Program Files (x86)\\mpv\\mpv.exe"],
    },
    Preset {
        id: "vlc",
        name: "VLC",
        kind: "player",
        args: &["--fullscreen", "--play-and-exit", "{file}"],
        platforms: &[VIDEOS],
        candidates: &["C:\\Program Files\\VideoLAN\\VLC\\vlc.exe", "C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe"],
    },
    Preset {
        id: "retroarch-snes",
        name: "RetroArch (Snes9x)",
        kind: "emulator",
        // The core path is relative to RetroArch's folder, which build_launch()
        // uses as the working directory.
        args: &["-L", "cores\\snes9x_libretro.dll", "-f", "{file}"],
        platforms: &["snes"],
        candidates: RETROARCH_CANDIDATES,
    },
    Preset {
        id: "retroarch-gba",
        name: "RetroArch (mGBA)",
        kind: "emulator",
        args: &["-L", "cores\\mgba_libretro.dll", "-f", "{file}"],
        platforms: &["gba"],
        candidates: RETROARCH_CANDIDATES,
    },
    Preset {
        id: "dolphin",
        name: "Dolphin",
        kind: "emulator",
        // -b: batch mode (no main window, and Dolphin exits when the game is
        // closed, so the launch is tracked); -e: the file to run. Both are
        // Dolphin's documented command-line flags.
        args: &["-b", "-e", "{file}"],
        platforms: &["gamecube", "wii"],
        candidates: &["C:\\Program Files\\Dolphin\\Dolphin.exe", "C:\\Program Files\\Dolphin-x64\\Dolphin.exe"],
    },
];

/// One profile as profiles.json stores it. sanitize() writes exactly these
/// six keys, so there are no unknown keys to carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub exe: String,
    pub args: Vec<String>,
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProfilesData {
    pub profiles: Vec<Profile>,
    /// Platform id (or `videos`) -> profile id; values are always strings.
    pub defaults: Map<String, Value>,
}

/// The program, arguments and working directory to start for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub exe: String,
    pub args: Vec<String>,
    pub cwd: String,
}

/// Returns a list of problems with a profile; empty means it can be saved.
/// Takes raw JSON because this is what decides whether a profile from a
/// file, a backup or the page is usable at all.
pub fn validate_profile(p: &Value) -> Vec<&'static str> {
    // An array passes this check in the JS too (typeof 'object'), then fails every field.
    if !p.is_object() && !p.is_array() {
        return vec!["Profile is missing"];
    }
    let field = |key: &str| p.as_object().and_then(|o| o.get(key));
    let text = |key: &str| field(key).and_then(Value::as_str);
    let list_of = |key: &str, ok: fn(&str) -> bool| {
        field(key).and_then(Value::as_array).is_some_and(|a| a.iter().all(|x| x.as_str().is_some_and(ok)))
    };

    let mut errors = Vec::new();
    if !text("id").is_some_and(|id| is_slug(id, ID_MAX)) {
        errors.push("Id must be lower-case letters, digits and dashes");
    }
    if !text("name").is_some_and(|n| !js_trim(n).is_empty()) {
        errors.push("Name is required");
    }
    if !text("kind").is_some_and(|k| KINDS.contains(&k)) {
        errors.push("Kind must be player or emulator");
    }
    match text("exe").filter(|exe| win_path::is_absolute(exe)) {
        None => errors.push("Program must be a full path"),
        // Windows only runs these through cmd.exe, which would interpret & and %
        // in file names. Scripted setups belong in a normal Launchpad item.
        Some(exe) if ends_with_ignore_case(exe, ".bat") || ends_with_ignore_case(exe, ".cmd") => {
            errors.push("Batch files (.bat, .cmd) cannot be used as a player or emulator")
        }
        Some(exe) if !ends_with_ignore_case(exe, ".exe") => errors.push("Program must be an .exe"),
        Some(_) => {}
    }
    if !list_of("args", |_| true) {
        errors.push("Arguments must be a list of text");
    }
    if !list_of("platforms", |x| is_slug(x, PLATFORM_MAX)) {
        errors.push("Platforms must be a list of platform ids");
    }
    errors
}

// Replaces {file}, {dir} and {name} in one pass, so a file name containing
// `$&`, `%` or even `{dir}` is inserted literally and never looked at again.
fn fill(arg: &str, values: &[(&str, &str); 3]) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        match values.iter().find(|(key, _)| rest.starts_with(key)) {
            Some((key, value)) => {
                out.push_str(value);
                rest = &rest[key.len()..];
            }
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// The program, arguments and working directory to spawn for one file.
pub fn build_launch(profile: &Profile, file: &str) -> Result<Launch> {
    if let Some(first) = validate_profile(&serde_json::to_value(profile)?).first() {
        return Err(Error::msg(*first));
    }
    if file.is_empty() {
        return Err(Error::msg("No file to open"));
    }
    let values = [("{file}", file), ("{dir}", win_path::dirname(file)), ("{name}", win_path::stem(file))];
    Ok(Launch {
        exe: profile.exe.clone(),
        args: profile.args.iter().map(|a| fill(a, &values)).collect(),
        cwd: win_path::dirname(&profile.exe).to_string(),
    })
}

/// A preset as a saveable profile, with the program the user picked.
pub fn from_preset(preset_id: &str, exe: &str) -> Result<Profile> {
    let preset = PRESETS.iter().find(|p| p.id == preset_id).ok_or_else(|| Error::msg(format!("Unknown preset: {preset_id}")))?;
    let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
    Ok(Profile {
        id: preset.id.into(),
        name: preset.name.into(),
        kind: preset.kind.into(),
        exe: exe.into(),
        args: owned(preset.args),
        platforms: owned(preset.platforms),
    })
}

/// Presets whose program is installed in one of its usual places.
pub fn detect_installed(exists: impl Fn(&str) -> bool) -> Vec<Profile> {
    PRESETS
        .iter()
        .filter_map(|preset| {
            let exe = preset.candidates.iter().find(|c| exists(c))?;
            from_preset(preset.id, exe).ok()
        })
        .collect()
}

/// Keeps only valid profiles and defaults that point at one of them.
pub fn sanitize(data: &Value) -> ProfilesData {
    let mut out = ProfilesData::default();
    let Some(data) = data.as_object() else { return out };
    let mut seen = HashSet::new();
    for p in data.get("profiles").and_then(Value::as_array).into_iter().flatten() {
        if !validate_profile(p).is_empty() {
            continue;
        }
        let Ok(profile) = serde_json::from_value::<Profile>(p.clone()) else { continue };
        if seen.insert(profile.id.clone()) {
            out.profiles.push(profile);
        }
    }
    let mut keep = |platform: &str, id: &Value| {
        if is_slug(platform, PLATFORM_MAX) && id.as_str().is_some_and(|id| seen.contains(id)) {
            out.defaults.insert(platform.to_string(), id.clone());
        }
    };
    match data.get("defaults") {
        Some(Value::Object(defaults)) => defaults.iter().for_each(|(platform, id)| keep(platform, id)),
        // The JS walks any object with Object.entries, so a list counts, keyed by index.
        Some(Value::Array(defaults)) => defaults.iter().enumerate().for_each(|(i, id)| keep(&i.to_string(), id)),
        _ => {}
    }
    out
}

/// Like settings.json: a missing file is empty, and an unreadable one is
/// moved aside once so the next save starts fresh instead of failing forever.
/// Locked files fail, so a save never replaces data it couldn't read.
pub fn load(dir: &Path) -> Result<ProfilesData> {
    let file = profiles_file(dir);
    let raw = match fs::read(&file) {
        Ok(raw) => raw,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ProfilesData::default()),
        Err(e) => return Err(e.into()),
    };
    if let Ok(parsed) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&raw)) {
        return Ok(sanitize(&parsed));
    }
    let corrupt_copy = corrupt_copy_path(&file);
    fs::rename(&file, &corrupt_copy)?;
    eprintln!("profiles.json is unreadable; moved it to {}", corrupt_copy.display());
    Ok(ProfilesData::default())
}

/// Cleans `data` (anything: the page's, a backup's) and writes what is left.
pub fn save(dir: &Path, data: &Value) -> Result<ProfilesData> {
    let clean = sanitize(data);
    fs::create_dir_all(dir)?;
    write_file_atomic(&profiles_file(dir), serde_json::to_string_pretty(&clean)?)?;
    Ok(clean)
}

/// The profile for a library entry: its own, else its platform's default
/// (videos use the 'videos' default). None when none is set up.
pub fn profile_for<'a>(entry: &Entry, data: &'a ProfilesData) -> Option<&'a Profile> {
    let by_id = |id: Option<&str>| data.profiles.iter().find(|p| Some(p.id.as_str()) == id);
    let own = entry.0.get("profile");
    if truthy(own) {
        return by_id(own.and_then(Value::as_str));
    }
    let key = if entry.library() == "videos" { VIDEOS } else { entry.platform()? };
    by_id(data.defaults.get(key).and_then(Value::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{scan, test_entry};
    use crate::test_util::{list_dir, temp_dir};
    use serde_json::json;

    const MPV: &str = "C:\\Program Files\\mpv\\mpv.exe";

    fn profile(extra: Value) -> Value {
        let mut p = json!({ "id": "mpv", "name": "mpv", "kind": "player", "exe": MPV, "args": ["--fs", "{file}"], "platforms": ["videos"] });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    }

    fn typed(extra: Value) -> Profile {
        serde_json::from_value(profile(extra)).unwrap()
    }

    #[test]
    fn build_launch_passes_the_file_as_one_argument_however_it_is_named() {
        let names = [
            "C:\\Movies\\Blade Runner (2017).mkv",
            "C:\\Movies\\Say \"Hi\".mkv",
            "C:\\ROMs\\Tom & Jerry.sfc",
            "C:\\ROMs\\100% Orange.sfc",
            "C:\\ROMs\\Cash $& $1 $$.sfc",
        ];
        for file in names {
            let launch = build_launch(&typed(json!({})), file).unwrap();
            assert_eq!(launch.exe, MPV);
            assert_eq!(launch.args, ["--fs", file]);
        }
    }

    #[test]
    fn build_launch_fills_dir_and_name_inside_arguments_and_runs_from_the_program_folder() {
        let p = typed(json!({ "args": ["--title={name}", "--cd", "{dir}", "{file}"] }));
        let out = build_launch(&p, "D:\\Shows\\S01\\Pilot.mkv").unwrap();
        assert_eq!(out.args, ["--title=Pilot", "--cd", "D:\\Shows\\S01", "D:\\Shows\\S01\\Pilot.mkv"]);
        assert_eq!(out.cwd, "C:\\Program Files\\mpv");
    }

    #[test]
    fn build_launch_leaves_unknown_braces_alone() {
        let out = build_launch(&typed(json!({ "args": ["{core}", "{file}"] })), "C:\\a.sfc").unwrap();
        assert_eq!(out.args, ["{core}", "C:\\a.sfc"]);
    }

    #[test]
    fn build_launch_refuses_an_invalid_profile_or_a_missing_file() {
        let bat = build_launch(&typed(json!({ "exe": "C:\\run.bat" })), "C:\\a.mkv").unwrap_err();
        assert!(bat.to_string().contains("Batch files"));
        assert!(build_launch(&typed(json!({})), "").unwrap_err().to_string().contains("No file"));
    }

    #[test]
    fn validate_profile_rejects_batch_files_relative_paths_and_non_exe_programs() {
        assert!(validate_profile(&profile(json!({}))).is_empty());
        for exe in ["C:\\x\\run.bat", "C:\\x\\RUN.CMD"] {
            assert!(validate_profile(&profile(json!({ "exe": exe })))[0].contains("Batch files"));
        }
        assert!(validate_profile(&profile(json!({ "exe": "mpv.exe" })))[0].contains("full path"));
        assert!(validate_profile(&profile(json!({ "exe": "C:\\x\\play.ps1" })))[0].contains(".exe"));
    }

    #[test]
    fn validate_profile_checks_id_name_kind_args_and_platforms() {
        let bad = |extra: Value| !validate_profile(&profile(extra)).is_empty();
        assert!(bad(json!({ "id": "../x" })));
        assert!(bad(json!({ "name": " " })));
        assert!(bad(json!({ "kind": "store" })));
        assert!(bad(json!({ "args": "--fs {file}" })));
        assert!(bad(json!({ "args": ["--fs", 3] })));
        assert!(bad(json!({ "platforms": ["Super Nintendo"] })));
        assert!(!validate_profile(&Value::Null).is_empty());
    }

    #[test]
    fn every_preset_is_valid_once_a_program_is_chosen() {
        for preset in PRESETS {
            let p = from_preset(preset.id, "C:\\Apps\\prog.exe").unwrap();
            let json = serde_json::to_value(&p).unwrap();
            assert!(validate_profile(&json).is_empty(), "{}", preset.id);
            assert!(json.get("candidates").is_none());
        }
        assert!(from_preset("nope", MPV).unwrap_err().to_string().contains("Unknown preset"));
    }

    #[test]
    fn from_preset_copies_so_editing_a_profile_never_changes_the_preset() {
        let mut p = from_preset("mpv", MPV).unwrap();
        p.args.push("--mute".into());
        assert_eq!(PRESETS.iter().find(|x| x.id == "mpv").unwrap().args, ["--fs", "{file}"]);
    }

    #[test]
    fn detect_installed_returns_presets_whose_program_exists() {
        let found = detect_installed(|p| p == "C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe");
        let pairs: Vec<_> = found.iter().map(|p| (p.id.as_str(), p.exe.as_str())).collect();
        assert_eq!(pairs, [("vlc", "C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe")]);
    }

    #[test]
    fn dolphin_and_retroarch_mgba_presets_sit_beside_the_snes_one_on_platform_ids_the_scanner_uses() {
        let by_id = |id: &str| PRESETS.iter().find(|p| p.id == id).unwrap();
        by_id("retroarch-snes"); // the SNES preset is kept
        assert_eq!(by_id("dolphin").platforms, ["gamecube", "wii"]);
        assert_eq!(by_id("dolphin").args, ["-b", "-e", "{file}"]);
        assert_eq!(by_id("retroarch-gba").platforms, ["gba"]);
        assert!(by_id("retroarch-gba").args.contains(&"cores\\mgba_libretro.dll"));
        for preset in PRESETS {
            for id in preset.platforms {
                assert!(*id == VIDEOS || scan::platform_by_id(id).is_some(), "{}: {id}", preset.id);
            }
        }
    }

    #[test]
    fn an_installed_retroarch_is_offered_as_both_presets_and_dolphin_is_found_in_program_files() {
        let found = detect_installed(|p| p == "C:\\RetroArch-Win64\\retroarch.exe" || p == "C:\\Program Files\\Dolphin\\Dolphin.exe");
        let mut ids: Vec<_> = found.iter().map(|p| p.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["dolphin", "retroarch-gba", "retroarch-snes"]);
        // `candidates` can't be on a Profile: the type has no such field.
    }

    #[test]
    fn missing_profiles_json_loads_as_empty_and_save_round_trips() {
        let dir = temp_dir();
        assert_eq!(load(dir.path()).unwrap(), ProfilesData::default());
        let data = json!({ "profiles": [profile(json!({}))], "defaults": { "videos": "mpv" } });
        save(dir.path(), &data).unwrap();
        assert_eq!(serde_json::to_value(load(dir.path()).unwrap()).unwrap(), data);
        assert!(profiles_file(dir.path()).exists());
    }

    #[test]
    fn save_drops_invalid_and_duplicate_profiles_and_defaults_that_point_nowhere() {
        let dir = temp_dir();
        let saved = save(
            dir.path(),
            &json!({
                "profiles": [profile(json!({})), profile(json!({ "name": "dup" })), profile(json!({ "id": "bad", "exe": "C:\\a.bat" }))],
                "defaults": { "videos": "mpv", "snes": "missing", "Bad Key": "mpv" },
            }),
        )
        .unwrap();
        assert_eq!(saved.profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["mpv"]);
        assert_eq!(Value::Object(saved.defaults), json!({ "videos": "mpv" }));
    }

    #[test]
    fn an_unreadable_profiles_json_is_moved_aside_once_and_loads_as_empty() {
        let dir = temp_dir();
        let file = profiles_file(dir.path());
        fs::write(&file, "not json").unwrap();
        assert_eq!(load(dir.path()).unwrap(), ProfilesData::default());
        assert!(!file.exists());
        assert_eq!(list_dir(dir.path()).iter().filter(|f| f.starts_with("profiles.corrupt-")).count(), 1);
        save(dir.path(), &json!({ "profiles": [profile(json!({}))], "defaults": {} })).unwrap();
        assert_eq!(load(dir.path()).unwrap().profiles.len(), 1);
    }

    #[test]
    fn profile_for_uses_the_entry_profile_then_the_platform_or_videos_default() {
        let ra = profile(json!({ "id": "ra", "kind": "emulator", "exe": "C:\\RA\\retroarch.exe", "platforms": ["snes"] }));
        let data = sanitize(&json!({ "profiles": [profile(json!({})), ra], "defaults": { "videos": "mpv", "snes": "ra" } }));
        let pick = |entry: Value| profile_for(&test_entry(entry), &data).map(|p| p.id.as_str());
        assert_eq!(pick(json!({ "library": "games", "platform": "snes", "profile": null })), Some("ra"));
        assert_eq!(pick(json!({ "library": "videos", "platform": null, "profile": null })), Some("mpv"));
        assert_eq!(pick(json!({ "library": "games", "platform": "snes", "profile": "mpv" })), Some("mpv"));
        assert_eq!(pick(json!({ "library": "games", "platform": "psx", "profile": null })), None);
        assert_eq!(pick(json!({ "library": "games", "platform": "snes", "profile": "gone" })), None);
    }
}
