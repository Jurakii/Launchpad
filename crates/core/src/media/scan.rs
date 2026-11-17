//! Finds games and videos in a library folder and merges them into the
//! library (library.rs). The walk is the only part that touches the disk;
//! detection, naming and merging are plain functions so they can be tested
//! without a real ROM collection.
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use super::library::{self, Entry};
use super::{js_is_space, win_path};

pub struct Platform {
    pub id: &'static str,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub exts: &'static [&'static str],
}

const fn platform(id: &'static str, name: &'static str, aliases: &'static [&'static str], exts: &'static [&'static str]) -> Platform {
    Platform { id, name, aliases, exts }
}

// Folder aliases are compared after lower-casing and dropping everything
// but letters and digits, so "Nintendo 64", "nintendo-64" and "N64" match.
// Extensions shared by several systems (.iso, .chd, .cue, .bin) are left
// out: those platforms are only found by folder name or a user pick.
pub const PLATFORMS: &[Platform] = &[
    platform("nes", "NES", &["nes", "famicom", "nintendoentertainmentsystem"], &[".nes", ".fds"]),
    platform("snes", "SNES", &["snes", "superfamicom", "sfc", "supernintendo"], &[".sfc", ".smc"]),
    platform("n64", "N64", &["n64", "nintendo64"], &[".n64", ".z64", ".v64"]),
    platform("gb", "Game Boy", &["gb", "gameboy"], &[".gb"]),
    platform("gbc", "Game Boy Color", &["gbc", "gameboycolor"], &[".gbc"]),
    platform("gba", "Game Boy Advance", &["gba", "gameboyadvance"], &[".gba"]),
    platform("nds", "Nintendo DS", &["nds", "ds", "nintendods"], &[".nds"]),
    platform("genesis", "Genesis", &["genesis", "megadrive", "md"], &[".md", ".gen", ".smd"]),
    platform("mastersystem", "Master System", &["mastersystem", "sms"], &[".sms"]),
    platform("psx", "PS1", &["psx", "ps1", "playstation", "playstation1"], &[]),
    platform("ps2", "PS2", &["ps2", "playstation2"], &[]),
    platform("psp", "PSP", &["psp"], &[".cso"]),
    platform("gamecube", "GameCube", &["gamecube", "gc", "ngc"], &[".gcm", ".rvz"]),
    platform("wii", "Wii", &["wii"], &[".wbfs"]),
    platform("saturn", "Saturn", &["saturn", "segasaturn"], &[]),
    platform("dreamcast", "Dreamcast", &["dreamcast", "dc"], &[".gdi", ".cdi"]),
    platform("arcade", "Arcade", &["arcade", "mame", "fbneo"], &[]),
];

// Files any game platform may use, on top of its own extensions.
pub const SHARED_GAME_EXTS: [&str; 9] = [".iso", ".chd", ".cue", ".bin", ".img", ".m3u", ".zip", ".7z", ".pbp"];
pub const VIDEO_EXTS: [&str; 10] = [".mkv", ".mp4", ".avi", ".mov", ".m4v", ".webm", ".wmv", ".mpg", ".mpeg", ".ts"];
const MAX_DEPTH: usize = 8;
const SKIP_DIRS: [&str; 5] = ["$recycle.bin", "system volume information", "node_modules", "downloaded_media", "media"];

fn key_of(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

fn same_path(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

pub fn platform_by_id(id: &str) -> Option<&'static Platform> {
    PLATFORMS.iter().find(|p| p.id == id)
}

/// The platform for a file, from the nearest folder whose name is an alias,
/// else from an extension only one platform uses. `dirs` are the folder
/// names from the library root down to the file.
pub fn detect_platform<S: AsRef<str>>(dirs: &[S], ext: &str) -> Option<&'static str> {
    for dir in dirs.iter().rev() {
        let key = key_of(dir.as_ref());
        if let Some(hit) = PLATFORMS.iter().find(|p| p.aliases.contains(&key.as_str())) {
            return Some(hit.id);
        }
    }
    let ext = ext.to_lowercase();
    let mut by_ext = PLATFORMS.iter().filter(|p| p.exts.contains(&ext.as_str()));
    match (by_ext.next(), by_ext.next()) {
        (Some(only), None) => Some(only.id),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanName {
    pub name: String,
    pub year: Option<u32>,
}

// /[([]((?:19|20)\d{2})[)\]]/: the first bracketed 19xx or 20xx.
fn bracketed_year(s: &str) -> Option<u32> {
    s.as_bytes().windows(6).find_map(|w| {
        let shaped = matches!(w[0], b'(' | b'[')
            && matches!(&w[1..3], b"19" | b"20")
            && w[3].is_ascii_digit()
            && w[4].is_ascii_digit()
            && matches!(w[5], b')' | b']');
        // The four digits are ASCII, so this slice is valid UTF-8.
        if shaped { std::str::from_utf8(&w[1..5]).ok()?.parse().ok() } else { None }
    })
}

// .replace(/\([^)]*\)|\[[^\]]*\]/g, ' '): each (...) or [...] becomes a
// space; a bracket that is never closed stays.
fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find(['(', '[']) {
        out.push_str(&rest[..open]);
        let close = if rest[open..].starts_with('(') { ')' } else { ']' };
        match rest[open + 1..].find(close) {
            Some(len) => {
                out.push(' ');
                rest = &rest[open + 1 + len + 1..];
            }
            None => {
                out.push_str(&rest[open..open + 1]);
                rest = &rest[open + 1..];
            }
        }
    }
    out + rest
}

/// "Chrono Trigger (USA) [!].sfc" -> Chrono Trigger. For videos a trailing
/// year is kept apart: "Blade Runner 2049 (2017).mkv" -> name + year 2017.
pub fn clean_name(file: &str) -> CleanName {
    let raw = win_path::stem(file);
    let year = bracketed_year(raw);
    // .replace(/[_.]+/g, ' ').replace(/\s+/g, ' ').trim()
    let spaced = strip_brackets(raw).replace(['_', '.'], " ");
    let base = spaced.split(js_is_space).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    CleanName { name: if base.is_empty() { raw.to_string() } else { base }, year }
}

fn is_game_file(ext: &str, platform: Option<&str>) -> bool {
    SHARED_GAME_EXTS.contains(&ext) || PLATFORMS.iter().any(|p| p.exts.contains(&ext) && platform.is_none_or(|id| p.id == id))
}

/// Disc images come as a playlist or cue sheet plus the track files it
/// lists. Only the top file is a game: in a folder with an .m3u, the .cue,
/// .bin and .img files are tracks; with a .cue, the .bin and .img are.
pub fn drop_track_files(files: Vec<String>) -> Vec<String> {
    let ext = |f: &str| win_path::extname(f).to_lowercase();
    let exts: HashSet<String> = files.iter().map(|f| ext(f)).collect();
    let tracks: &[&str] = if exts.contains(".m3u") {
        &[".cue", ".bin", ".img", ".chd", ".iso"]
    } else if exts.contains(".cue") || exts.contains(".gdi") {
        &[".bin", ".img", ".raw"]
    } else {
        &[]
    };
    files.into_iter().filter(|f| !tracks.contains(&ext(f).as_str())).collect()
}

/// One file a scan found. `folder` is the subfolder relative to the root
/// ('' for the root itself).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Found {
    pub target: String,
    pub name: String,
    pub year: Option<u32>,
    pub platform: Option<String>,
    pub folder: String,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// `games` or `videos`.
    pub library: String,
    pub recursive: bool,
    /// The user's picks: `{ "<relative folder>": platformId }`; '' = root.
    pub platforms: Map<String, Value>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { library: "games".into(), recursive: true, platforms: Map::new() }
    }
}

enum Kind {
    File,
    Dir,
    Other,
}

// What Node's Dirent says on Windows: anything with a reparse point (a
// symlink, a junction, a cloud placeholder) is a link, so neither a file
// nor a folder, and loops can't happen.
fn kind_of(entry: &fs::DirEntry) -> Kind {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        match entry.metadata() {
            Ok(m) if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => return Kind::Other,
            Ok(_) => {}
            Err(_) => return Kind::Other,
        }
    }
    match entry.file_type() {
        Ok(t) if t.is_file() => Kind::File,
        Ok(t) if t.is_dir() => Kind::Dir,
        _ => Kind::Other,
    }
}

struct Walk<'a> {
    options: &'a ScanOptions,
    found: Vec<Found>,
}

impl Walk<'_> {
    // The user's pick for this folder or the nearest parent that has one.
    fn override_for(&self, rel: &str) -> Option<&str> {
        let mut r = rel;
        loop {
            let key = if r == "." { "" } else { r };
            if let Some((_, pick)) = self.options.platforms.iter().find(|(k, _)| same_path(k, key)) {
                return pick.as_str();
            }
            if key.is_empty() {
                return None;
            }
            r = win_path::dirname(r);
        }
    }

    fn walk(&mut self, dir: &Path, dirs: &mut Vec<String>) {
        // unreadable subfolder: skip it, keep the rest of the scan
        let Ok(dirents) = fs::read_dir(dir).and_then(|d| d.collect::<std::io::Result<Vec<_>>>()) else { return };
        let dirents: Vec<(String, Kind)> =
            dirents.iter().map(|d| (d.file_name().to_string_lossy().into_owned(), kind_of(d))).collect();

        let games = self.options.library == "games";
        let rel = dirs.join("\\");
        let files: Vec<String> = dirents.iter().filter(|(_, k)| matches!(k, Kind::File)).map(|(n, _)| n.clone()).collect();
        let kept = if games { drop_track_files(files) } else { files };
        for name in kept {
            let ext = win_path::extname(&name).to_lowercase();
            let target = dir.join(&name).to_string_lossy().into_owned();
            if self.options.library == "videos" {
                if VIDEO_EXTS.contains(&ext.as_str()) {
                    let CleanName { name, year } = clean_name(&name);
                    self.found.push(Found { target, name, year, platform: None, folder: rel.clone() });
                }
                continue;
            }
            let platform = self.override_for(&rel).filter(|p| !p.is_empty()).or_else(|| detect_platform(dirs, &ext));
            if !is_game_file(&ext, platform) {
                continue;
            }
            let found = Found { target, name: clean_name(&name).name, year: None, platform: platform.map(String::from), folder: rel.clone() };
            self.found.push(found);
        }
        if !self.options.recursive || dirs.len() >= MAX_DEPTH {
            return;
        }
        for (name, kind) in dirents {
            if !matches!(kind, Kind::Dir) || name.starts_with('.') || SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                continue;
            }
            let sub = dir.join(&name);
            dirs.push(name);
            self.walk(&sub, dirs);
            dirs.pop();
        }
    }
}

/// Walks a library folder, in the order the file system lists it: a
/// folder's files first, then its subfolders.
pub fn scan_folder(root: &Path, options: &ScanOptions) -> Vec<Found> {
    // path.join() tidied the root on every call: forward slashes, doubled
    // and trailing separators.
    let root: PathBuf = root.components().collect();
    let mut walk = Walk { options, found: Vec::new() };
    walk.walk(&root, &mut Vec::new());
    walk.found
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub folder: String,
    pub platform: Option<String>,
    pub count: usize,
}

// Stands in for String.prototype.localeCompare (ICU, default locale): the
// ASCII order below is what ICU gives - punctuation, digits, then letters
// with case as a tie-break, lower first.
// shortcut: other characters sort after ASCII by code point (ICU folds
// accents: "é" beside "e"); only the order of the Add library folder
// screen's rows depends on it. Use a collation crate if that matters.
fn locale_compare(a: &str, b: &str) -> Ordering {
    const ASCII: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";
    let rank = |c: char| {
        let lower = c.to_lowercase().next().unwrap_or(c);
        ASCII.find(lower).map_or(1000 + lower as u32, |i| i as u32)
    };
    a.chars().map(rank).cmp(b.chars().map(rank)).then_with(|| a.chars().map(char::is_uppercase).cmp(b.chars().map(char::is_uppercase)))
}

/// Counts per subfolder and platform, for the Add library folder screen.
pub fn summarize(found: &[Found]) -> Vec<Summary> {
    let mut groups: Vec<(String, Summary)> = Vec::new();
    for f in found {
        let key = format!("{}\0{}", f.folder.to_lowercase(), f.platform.as_deref().unwrap_or(""));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.count += 1,
            None => groups.push((key, Summary { folder: f.folder.clone(), platform: f.platform.clone(), count: 1 })),
        }
    }
    let mut out: Vec<Summary> = groups.into_iter().map(|(_, g)| g).collect();
    out.sort_by(|a, b| locale_compare(&a.folder, &b.folder));
    out
}

// Whether `file` is below `root`, ignoring case. Stands in for the JS's
// path.relative() test, quirk included: a first folder whose name starts
// with ".." counts as outside.
// shortcut: both paths are taken as full paths without "." or ".." parts,
// which is what scans and the folder picker produce; path.relative() also
// resolved relative ones against the working directory.
fn is_inside(file: &str, root: &str) -> bool {
    let tidy = |p: &str| {
        let parts: Vec<String> = p.split(['/', '\\']).filter(|s| !s.is_empty()).map(str::to_lowercase).collect();
        parts.join("\\")
    };
    let (file, root) = (tidy(file), tidy(root));
    file.strip_prefix(&root).and_then(|rest| rest.strip_prefix('\\')).is_some_and(|rest| !rest.starts_with(".."))
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergeResult {
    pub entries: Vec<Entry>,
    pub added: usize,
    pub missing: usize,
    pub restored: usize,
}

/// Merges one scan of `root` into the library list. Files already there keep
/// their id, art and play counts (matched by path, ignoring case); files no
/// longer found are marked unavailable rather than removed; files that came
/// back are available again. Entries outside root are never touched.
/// Returns a new list plus counts.
pub fn merge_scan(entries: &[Entry], found: &[Found], root: &str, library: &str, now: i64) -> MergeResult {
    let by_path: HashSet<String> = found.iter().map(|f| f.target.to_lowercase()).collect();
    let (mut added, mut missing, mut restored) = (0, 0, 0);
    let mut seen = HashSet::new();

    let mut next: Vec<Entry> = entries
        .iter()
        .map(|e| {
            let target = e.0.get("target").and_then(Value::as_str);
            // Single apps (type path) were added on their own, not by this folder's scan.
            let Some(target) = target.filter(|t| e.library() == library && e.launch_type() != "path" && is_inside(t, root)) else {
                return e.clone();
            };
            let key = target.to_lowercase();
            let here = by_path.contains(&key);
            seen.insert(key);
            let mut e = e.clone();
            if !here && e.available() {
                missing += 1;
                e.0.insert("available".into(), false.into());
            } else if here && !e.available() {
                restored += 1;
                e.0.insert("available".into(), true.into());
            }
            e
        })
        .collect();

    for f in found {
        if !seen.insert(f.target.to_lowercase()) {
            continue;
        }
        added += 1;
        let mut meta = Map::new();
        if let Some(year) = f.year.filter(|y| *y != 0) {
            meta.insert("year".into(), year.into());
        }
        let mut entry = Map::new();
        entry.insert("id".into(), library::new_id().into());
        entry.insert("library".into(), library.into());
        entry.insert("name".into(), f.name.as_str().into());
        entry.insert("platform".into(), f.platform.as_deref().map_or(Value::Null, Value::from));
        entry.insert("type".into(), "profile".into());
        entry.insert("target".into(), f.target.as_str().into());
        entry.insert("createdAt".into(), now.into());
        entry.insert("meta".into(), Value::Object(meta));
        next.push(library::normalize(entry));
    }
    MergeResult { entries: next, added, missing, restored }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::test_entry;
    use crate::test_util::{temp_dir, TempDir};
    use serde_json::json;

    struct Setup {
        _dir: TempDir,
        root: PathBuf,
    }

    impl Setup {
        // Creates empty files at the given paths under root.
        fn touch(&self, rels: &[&str]) {
            for rel in rels {
                let file = self.root.join(rel);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, "").unwrap();
            }
        }
    }

    fn setup() -> Setup {
        let dir = temp_dir();
        let root = dir.path().join("lib");
        Setup { _dir: dir, root }
    }

    fn names(found: &[Found]) -> Vec<&str> {
        let mut names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        names.sort();
        names
    }

    fn by_name<'a>(found: &'a [Found], name: &str) -> &'a Found {
        found.iter().find(|f| f.name == name).unwrap()
    }

    fn clean(name: &str, year: Option<u32>) -> CleanName {
        CleanName { name: name.into(), year }
    }

    #[test]
    fn detect_platform_nearest_folder_alias_wins_then_a_unique_extension() {
        assert_eq!(detect_platform(&["Nintendo 64"], ".zip"), Some("n64"));
        assert_eq!(detect_platform(&["Super Famicom"], ".sfc"), Some("snes"));
        assert_eq!(detect_platform(&["psx", "Disc Games"], ".chd"), Some("psx"));
        assert_eq!(detect_platform(&["snes", "n64"], ".sfc"), Some("n64"), "nearest folder beats extension");
        assert_eq!(detect_platform(&["misc"], ".SMC"), Some("snes"));
        assert_eq!(detect_platform(&["misc"], ".iso"), None);
        assert_eq!(detect_platform::<&str>(&[], ".zip"), None);
    }

    #[test]
    fn clean_name_strips_region_tags_and_keeps_a_year_apart() {
        assert_eq!(clean_name("Chrono Trigger (USA) [!].sfc"), clean("Chrono Trigger", None));
        assert_eq!(clean_name("Blade Runner 2049 (2017).mkv"), clean("Blade Runner 2049", Some(2017)));
        assert_eq!(clean_name("The_Expanse.S02E04.mkv"), clean("The Expanse S02E04", None));
        assert_eq!(clean_name("(USA).sfc"), clean("(USA)", None), "never an empty name");
    }

    #[test]
    fn drop_track_files_keeps_the_playlist_or_cue_sheet_not_its_tracks() {
        let drop = |files: &[&str]| drop_track_files(files.iter().map(|f| f.to_string()).collect());
        assert_eq!(drop(&["G.cue", "G (Track 1).bin", "G (Track 2).bin"]), ["G.cue"]);
        assert_eq!(drop(&["G.m3u", "G (Disc 1).cue", "G (Disc 1).bin", "G (Disc 2).chd"]), ["G.m3u"]);
        assert_eq!(drop(&["A.bin", "B.iso"]), ["A.bin", "B.iso"]);
    }

    #[test]
    fn scan_folder_finds_games_by_folder_and_extension_and_skips_other_files() {
        let s = setup();
        s.touch(&[
            "snes/Chrono Trigger (USA).sfc",
            "snes/readme.txt",
            "psx/Final Fantasy VII/FF7.cue",
            "psx/Final Fantasy VII/FF7 (Track 1).bin",
            "misc/Earthbound.smc",
            "misc/mystery.iso",
            "snes/.hidden/Secret.sfc",
            "snes/media/cover.png",
        ]);
        let found = scan_folder(&s.root, &ScanOptions::default());
        assert_eq!(names(&found), ["Chrono Trigger", "Earthbound", "FF7", "mystery"]);
        let chrono = by_name(&found, "Chrono Trigger");
        assert_eq!(chrono.platform.as_deref(), Some("snes"));
        assert_eq!(chrono.folder, "snes");
        assert_eq!(by_name(&found, "FF7").platform.as_deref(), Some("psx"));
        assert_eq!(by_name(&found, "Earthbound").platform.as_deref(), Some("snes"));
        assert_eq!(by_name(&found, "mystery").platform, None);
        assert_eq!(Path::new(&chrono.target), s.root.join("snes").join("Chrono Trigger (USA).sfc"));
    }

    #[test]
    fn scan_folder_applies_the_user_platform_picks_to_a_folder_and_its_subfolders() {
        let s = setup();
        s.touch(&["misc/a.iso", "misc/deep/b.iso", "other/c.iso"]);
        let options = ScanOptions { platforms: json!({ "MISC": "ps2" }).as_object().unwrap().clone(), ..Default::default() };
        let found = scan_folder(&s.root, &options);
        let mut picks: Vec<_> = found.iter().map(|f| (f.name.as_str(), f.platform.as_deref())).collect();
        picks.sort();
        assert_eq!(picks, [("a", Some("ps2")), ("b", Some("ps2")), ("c", None)]);
    }

    #[test]
    fn scan_folder_without_subfolders_stays_at_the_top_level() {
        let s = setup();
        s.touch(&["Top.sfc", "snes/Deep.sfc"]);
        assert_eq!(names(&scan_folder(&s.root, &ScanOptions { recursive: false, ..Default::default() })), ["Top"]);
    }

    #[test]
    fn scan_folder_for_videos_takes_video_files_only_and_reads_the_year() {
        let s = setup();
        s.touch(&["Movies/Arrival (2016).mkv", "Movies/Arrival (2016).srt", "Movies/poster.jpg", "Shows/S01/Pilot.mp4"]);
        let found = scan_folder(&s.root, &ScanOptions { library: "videos".into(), ..Default::default() });
        assert_eq!(names(&found), ["Arrival", "Pilot"]);
        assert_eq!(by_name(&found, "Arrival").year, Some(2016));
        assert!(found.iter().all(|f| f.platform.is_none()));
    }

    #[test]
    fn scan_folder_on_a_missing_folder_finds_nothing_instead_of_throwing() {
        let s = setup();
        assert!(scan_folder(&s.root.join("nope"), &ScanOptions::default()).is_empty());
    }

    fn found(target: &str, name: &str, year: Option<u32>, platform: Option<&str>, folder: &str) -> Found {
        Found { target: target.into(), name: name.into(), year, platform: platform.map(String::from), folder: folder.into() }
    }

    #[test]
    fn summarize_groups_by_folder_and_platform() {
        let list = [found("", "", None, Some("snes"), "snes"), found("", "", None, Some("snes"), "snes"), found("", "", None, None, "misc")];
        assert_eq!(
            summarize(&list),
            [
                Summary { folder: "misc".into(), platform: None, count: 1 },
                Summary { folder: "snes".into(), platform: Some("snes".into()), count: 2 },
            ]
        );
    }

    #[test]
    fn merge_scan_adds_new_files_keeps_known_ones_and_marks_missing_ones_unavailable() {
        let root = "D:\\ROMs";
        let kept = test_entry(json!({ "id": "k", "library": "games", "name": "Kept", "target": "D:\\ROMs\\snes\\Kept.sfc", "available": true, "launchCount": 4 }));
        let gone = test_entry(json!({ "id": "g", "library": "games", "name": "Gone", "target": "D:\\ROMs\\snes\\Gone.sfc", "available": true }));
        let back = test_entry(json!({ "id": "b", "library": "games", "name": "Back", "target": "D:\\ROMs\\snes\\Back.sfc", "available": false }));
        let elsewhere = test_entry(json!({ "id": "e", "library": "games", "name": "Else", "target": "E:\\Other\\Else.sfc", "available": true }));
        let sibling = test_entry(json!({ "id": "s", "library": "games", "name": "Sib", "target": "D:\\ROMs2\\Sib.sfc", "available": true }));
        let video = test_entry(json!({ "id": "v", "library": "videos", "name": "Vid", "target": "D:\\ROMs\\Vid.mkv", "available": true }));
        let list = [
            found("d:\\roms\\SNES\\kept.sfc", "Kept", None, Some("snes"), ""),
            found("D:\\ROMs\\snes\\Back.sfc", "Back", None, Some("snes"), ""),
            found("D:\\ROMs\\snes\\New.sfc", "New", None, Some("snes"), ""),
            found("D:\\ROMs\\snes\\New.sfc", "New", None, Some("snes"), ""),
        ];

        let input = [kept.clone(), gone.clone(), back, elsewhere.clone(), sibling.clone(), video.clone()];
        let result = merge_scan(&input, &list, root, "games", 42);
        assert_eq!([result.added, result.missing, result.restored], [1, 1, 1]);
        let by_id = |id: &str| result.entries.iter().find(|e| e.id() == id).unwrap();
        // The JS checks these are the same objects; here they are equal copies.
        assert_eq!(by_id("k"), &kept, "unchanged entries are the same");
        assert!(!by_id("g").available());
        assert!(by_id("b").available());
        assert_eq!(by_id("e"), &elsewhere);
        assert_eq!(by_id("s"), &sibling, "D:\\ROMs2 is not inside D:\\ROMs");
        assert_eq!(by_id("v"), &video, "other libraries are untouched");

        let added = result.entries.iter().find(|e| e.name() == "New").unwrap();
        assert!(library::is_valid_id(added.id()));
        assert_eq!(
            json!([added.0["library"], added.0["platform"], added.0["type"], added.0["available"], added.0["createdAt"]]),
            json!(["games", "snes", "profile", true, 42])
        );
        assert_eq!(input[1], gone, "the input list is not mutated");
    }

    #[test]
    fn merge_scan_stores_a_video_year_in_meta() {
        let list = [found("D:\\Movies\\Arrival (2016).mkv", "Arrival", Some(2016), None, "")];
        let entries = merge_scan(&[], &list, "D:\\Movies", "videos", 0).entries;
        assert_eq!(entries[0].0["meta"], json!({ "year": 2016 }));
        assert_eq!(entries[0].library(), "videos");
    }

    #[test]
    fn merge_scan_leaves_single_apps_alone_even_when_they_sit_inside_the_scanned_folder() {
        let app = test_entry(json!({ "id": "a", "library": "games", "name": "Tool", "type": "path", "target": "D:\\ROMs\\tools\\Tool.exe", "available": true }));
        let result = merge_scan(std::slice::from_ref(&app), &[], "D:\\ROMs", "games", 0);
        assert_eq!(result.entries[0], app);
        assert_eq!(result.missing, 0);
    }

    // Not in the JS suite: pins the hand-written stand-ins for its regexes,
    // path.relative() and localeCompare(). Expected values are from node.
    #[test]
    fn stand_ins_for_js_built_ins_match_node() {
        assert_eq!(clean_name("A [1999) (b.mkv"), clean("A [1999) (b", Some(1999)));
        assert_eq!(clean_name("a (b [c) d].mkv"), clean("a d]", None));
        assert_eq!(clean_name("[x (y] z).sfc"), clean("z)", None));
        assert_eq!(clean_name("(a(b)c).sfc"), clean("c)", None));
        assert_eq!(clean_name("X (1899) [2001].mkv").year, Some(2001));
        assert_eq!(clean_name("a\u{a0}_b..c (x) [y] d.z"), clean("a b c d", None));
        assert!(is_inside("D:\\ROMs\\snes\\a.sfc", "d:/roms/"));
        assert!(!is_inside("D:\\ROMs", "D:\\ROMs") && !is_inside("D:\\ROMs2\\a", "D:\\ROMs") && !is_inside("D:\\ROMs\\..foo\\a", "D:\\ROMs"));
        let mut folders = ["snes\\a", "snes", "Snes", "misc", "snes a", "SNES\\b"];
        folders.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(folders, ["misc", "snes", "Snes", "snes a", "snes\\a", "SNES\\b"]);
    }
}
