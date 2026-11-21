//! Importing games from other launchers into Media: Steam's installed games
//! and Quiver Launcher's installed apps. Each becomes a `path` entry that
//! runs the game's own program (in its folder), not the launcher, with the
//! launcher's art where it keeps some on disk.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::library::{new_id, normalize, Entry};
use super::win_path;

/// A game found in another launcher.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// "steam:<appid>" or "quiver:<id>": an entry made from it keeps this as
    /// `importedFrom`, so importing again skips it.
    pub key: String,
    pub name: String,
    /// The program, or a steam:// address when no program was found.
    pub target: String,
    /// Art files on disk, by Media slot.
    pub art: Vec<(&'static str, PathBuf)>,
}

impl Found {
    /// A Games entry for it.
    pub fn entry(&self, now: i64) -> Entry {
        let url = self.target.contains("://");
        let mut e = Map::new();
        e.insert("id".into(), new_id().into());
        e.insert("library".into(), "games".into());
        e.insert("name".into(), self.name.clone().into());
        e.insert("platform".into(), Value::Null);
        e.insert("type".into(), if url { "url" } else { "path" }.into());
        e.insert("profile".into(), Value::Null);
        e.insert("target".into(), self.target.clone().into());
        if !url {
            // Games look for their files next to themselves.
            if let Some(dir) = Path::new(&self.target).parent() {
                e.insert("cwd".into(), dir.to_string_lossy().into_owned().into());
            }
        }
        e.insert("importedFrom".into(), self.key.clone().into());
        e.insert("createdAt".into(), now.into());
        normalize(e)
    }
}

/// Whether `entries` already has this game (imported before, or added by hand).
pub fn already_in(entries: &[Entry], found: &Found) -> bool {
    entries.iter().any(|e| {
        e.0.get("importedFrom").and_then(Value::as_str) == Some(found.key.as_str()) || e.target().eq_ignore_ascii_case(&found.target)
    })
}

// ---------- Steam ----------

/// Valve's KeyValues text, flattened: every "key" "value" pair in file
/// order, at any depth. Enough for appmanifest and libraryfolders.
fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some(n) = chars.next() {
                                s.push(match n {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                            }
                        }
                        '"' => break,
                        c => s.push(c),
                    }
                }
                tokens.push(Some(s));
            }
            '{' | '}' => tokens.push(None),
            _ => {}
        }
    }
    let mut pairs = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        match (&tokens[i], &tokens[i + 1]) {
            (Some(k), Some(v)) => {
                pairs.push((k.clone(), v.clone()));
                i += 2;
            }
            _ => i += 1,
        }
    }
    pairs
}

fn first<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

/// Steam's tools that show up as installed apps.
fn steam_tool(appid: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    appid == "228980" || n.contains("redistributable") || n.starts_with("proton") || n.contains("steamvr") || n.contains("steam linux runtime") || n.contains("dedicated server")
}

/// The installed games of the Steam at `root` (the folder with steam.exe),
/// across all its library folders.
pub fn steam(root: &Path) -> Vec<Found> {
    let mut libraries = vec![root.to_path_buf()];
    if let Ok(text) = fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for (k, v) in vdf_pairs(&text) {
            if k == "path" && !libraries.iter().any(|l| l.as_os_str().eq_ignore_ascii_case(v.as_str())) {
                libraries.push(PathBuf::from(v));
            }
        }
    }
    let mut found = Vec::new();
    for lib in libraries {
        let apps = lib.join("steamapps");
        let Ok(dir) = fs::read_dir(&apps) else { continue };
        let mut manifests: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.starts_with("appmanifest_") && n.ends_with(".acf")
        }).collect();
        manifests.sort();
        for m in manifests {
            let Ok(text) = fs::read_to_string(&m) else { continue };
            let pairs = vdf_pairs(&text);
            let (Some(appid), Some(name), Some(installdir)) = (first(&pairs, "appid"), first(&pairs, "name"), first(&pairs, "installdir")) else {
                continue;
            };
            // StateFlags 4: fully installed.
            let installed = first(&pairs, "StateFlags").and_then(|f| f.parse::<u32>().ok()).is_some_and(|f| f & 4 != 0);
            if !installed || steam_tool(appid, name) || found.iter().any(|f: &Found| f.key == format!("steam:{appid}")) {
                continue;
            }
            let folder = apps.join("common").join(installdir);
            let target = main_exe(&folder, &[name, installdir]).map_or_else(|| format!("steam://rungameid/{appid}"), |p| p.to_string_lossy().into_owned());
            found.push(Found { key: format!("steam:{appid}"), name: name.to_string(), target, art: steam_art(root, appid) });
        }
    }
    found.sort_by_key(|f| f.name.to_lowercase());
    found
}

/// Steam's own copies of a game's library art: librarycache/<appid>/ (or a
/// hashed folder inside it), or the older flat <appid>_<file> names.
fn steam_art(root: &Path, appid: &str) -> Vec<(&'static str, PathBuf)> {
    let cache = root.join("appcache").join("librarycache");
    let folder = cache.join(appid);
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = fs::read_dir(&folder) {
        for e in dir.flatten() {
            let p = e.path();
            if p.is_dir() {
                files.extend(fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()));
            } else {
                files.push(p);
            }
        }
    }
    let find = |names: &[&str]| -> Option<PathBuf> {
        names.iter().find_map(|n| {
            files.iter().find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(n))).cloned().or_else(|| {
                let flat = cache.join(format!("{appid}_{n}"));
                flat.is_file().then_some(flat)
            })
        })
    };
    let mut art = Vec::new();
    for (slot, names) in [
        ("cover", &["library_600x900_2x.jpg", "library_600x900.jpg", "library_capsule.jpg"][..]),
        ("banner", &["library_hero.jpg"][..]),
        ("logo", &["logo.png"][..]),
        ("screenshot", &["library_header.jpg", "header.jpg"][..]),
    ] {
        if let Some(p) = find(names) {
            art.push((slot, p));
        }
    }
    art
}

/// Lowercase letters and digits only, for comparing names.
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Programs in a game folder that aren't the game.
fn not_the_game(stem: &str) -> bool {
    const SKIP: [&str; 23] = [
        "unins", "setup", "install", "redist", "vcredist", "vc_redist", "dxsetup", "directx", "dotnet", "crash", "report", "unitycrashhandler",
        "ue4prereq", "ueprereq", "easyanticheat", "battleye", "beservice", "cefprocess", "helper", "update", "touchup", "notification", "busybox",
    ];
    let s = stem.to_ascii_lowercase();
    SKIP.iter().any(|k| s.contains(k))
}

/// The game's program in `folder`: the .exe named most like the game (by
/// any of `names`), else the biggest one nearest the top. Looks three
/// folders deep.
pub fn main_exe(folder: &Path, names: &[&str]) -> Option<PathBuf> {
    let wanted: Vec<String> = names.iter().map(|n| squash(n)).filter(|n| !n.is_empty()).collect();
    // (name match, shallowest, biggest)
    type Score = (u8, std::cmp::Reverse<usize>, u64);
    let mut best: Option<(Score, PathBuf)> = None;
    let mut stack = vec![(folder.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let is_exe = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe"));
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !is_exe || not_the_game(stem) {
                continue;
            }
            let s = squash(stem);
            let name_score = if wanted.contains(&s) {
                3
            } else if wanted.iter().any(|w| !s.is_empty() && (w.contains(&s) || s.contains(w.as_str()))) {
                2
            } else {
                0
            };
            let score = (name_score, std::cmp::Reverse(depth), meta.len());
            if best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

// ---------- Quiver Launcher ----------

fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).ok()
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}

/// The installed apps of a Quiver Launcher, with the program each runs.
/// `dir` is its data folder (4.x: library.json and installs.json; a
/// portable copy keeps it in `data` beside the program) or the folder of a
/// 3.x copy (apps.json beside QuiverLauncher.exe, apps in `Apps`).
pub fn quiver(dir: &Path) -> Vec<Found> {
    if dir.join("installs.json").is_file() {
        quiver4(dir)
    } else if dir.join("data").join("installs.json").is_file() {
        quiver4(&dir.join("data"))
    } else {
        quiver3(dir)
    }
}

/// Whether `dir` holds a Quiver Launcher library `quiver` can read.
pub fn is_quiver(dir: &Path) -> bool {
    dir.join("installs.json").is_file()
        || dir.join("data").join("installs.json").is_file()
        || (dir.join("apps.json").is_file() && (dir.join("Apps").is_dir() || dir.join("QuiverLauncher.exe").is_file()))
}

/// 3.x: `{ "apps": [{ name, folderName, installPath }] }`, an app installed
/// when its folder is there (its installPath, or Apps\<folderName>, or the
/// appsPath its settings.json names).
fn quiver3(dir: &Path) -> Vec<Found> {
    let Some(doc) = ["apps.json", "games.json"].iter().find_map(|f| read_json(&dir.join(f))) else { return vec![] };
    let settings = read_json(&dir.join("settings.json")).unwrap_or(Value::Null);
    let mut roots = vec![dir.join("Apps")];
    if let Some(p) = text(&settings, "appsPath") {
        roots.insert(0, if win_path::is_absolute(p) { PathBuf::from(p) } else { dir.join(p) });
    }
    let list = doc.as_object().and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case("apps"))).and_then(|(_, v)| v.as_array());
    let mut found = Vec::new();
    for app in list.into_iter().flatten() {
        let Some(name) = text(app, "name") else { continue };
        let folder = text(app, "folderName").unwrap_or("");
        let install = text(app, "installPath")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .or_else(|| if folder.is_empty() { None } else { roots.iter().map(|r| r.join(folder)).find(|p| p.is_dir()) });
        let Some(install) = install else { continue };
        let project = text(app, "project").unwrap_or("");
        let Some(exe) = main_exe(&install, &[name, project, folder]) else { continue };
        let key = text(app, "repository").map_or_else(|| format!("quiver:{folder}"), |r| format!("quiver:github:{r}"));
        found.push(Found { key, name: name.to_string(), target: exe.to_string_lossy().into_owned(), art: vec![] });
    }
    found.sort_by_key(|f| f.name.to_lowercase());
    found
}

fn quiver4(data: &Path) -> Vec<Found> {
    let Some(Value::Object(installs)) = read_json(&data.join("installs.json")) else { return vec![] };
    let library = read_json(&data.join("library.json")).unwrap_or(Value::Null);
    let catalog = read_json(&data.join("catalog.json")).unwrap_or(Value::Null);
    // The apps folder: the one the player picked (apps-folder.json), else `apps` in the data folder.
    let apps_folder = read_json(&data.join("apps-folder.json"))
        .and_then(|v| text(&v, "path").map(PathBuf::from))
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| data.join("apps"));
    let mut found = Vec::new();
    for (id, install) in installs {
        let item = library.as_array().and_then(|l| l.iter().find(|i| text(i, "id") == Some(id.as_str()) && i.get("removed") != Some(&Value::Bool(true))));
        let Some(item) = item else { continue };
        let folder = text(&install, "folder").unwrap_or("");
        let dir = text(&install, "dir").map(PathBuf::from).unwrap_or_else(|| match text(&install, "group") {
            Some(g) => apps_folder.join(g).join(folder),
            None => apps_folder.join(folder),
        });
        let chosen = install.get("launch").and_then(|l| text(l, "executable"));
        let exe = chosen
            .map(|c| if win_path::is_absolute(c) { PathBuf::from(c) } else { dir.join(c) })
            .or_else(|| text(&install, "program").map(PathBuf::from))
            .or_else(|| install.get("executables")?.as_array()?.iter().find_map(Value::as_str).map(|e| dir.join(e)))
            .filter(|p| p.is_file())
            .or_else(|| main_exe(&dir, &[folder]));
        let Some(exe) = exe.filter(|p| p.is_file()) else { continue };
        let name = item
            .get("overrides")
            .and_then(|o| text(o, "name"))
            .or_else(|| catalog.get(&id).and_then(|c| text(c, "name")))
            .or_else(|| text(item, "slug"))
            .unwrap_or(folder)
            .to_string();
        let mut art = Vec::new();
        if let Some(cover) = item.get("overrides").and_then(|o| text(o, "cover")).map(PathBuf::from).filter(|p| p.is_file()) {
            art.push(("cover", cover));
        }
        found.push(Found { key: format!("quiver:{id}"), name, target: exe.to_string_lossy().into_owned(), art });
    }
    found.sort_by_key(|f| f.name.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_dir;
    use serde_json::json;

    #[test]
    fn vdf_pairs_reads_nested_keys_and_escapes() {
        let text = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"apps\" { \"10\" \"1\" }\n\t}\n}";
        let pairs = vdf_pairs(text);
        assert_eq!(first(&pairs, "path"), Some("C:\\Program Files (x86)\\Steam"));
        assert_eq!(first(&pairs, "10"), Some("1"));
    }

    #[test]
    fn steam_finds_installed_games_their_program_and_art_and_skips_tools() {
        let dir = temp_dir();
        let root = dir.path().join("Steam");
        let other = dir.path().join("Lib2");
        let apps = root.join("steamapps");
        fs::create_dir_all(&apps).unwrap();
        fs::create_dir_all(other.join("steamapps")).unwrap();
        let other_escaped = other.to_string_lossy().replace('\\', "\\\\");
        fs::write(apps.join("libraryfolders.vdf"), format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{other_escaped}\" }} }}")).unwrap();
        let manifest = |at: &Path, id: &str, name: &str, dirname: &str, flags: u32| {
            fs::write(at.join(format!("appmanifest_{id}.acf")), format!("\"AppState\" {{ \"appid\" \"{id}\" \"name\" \"{name}\" \"StateFlags\" \"{flags}\" \"installdir\" \"{dirname}\" }}")).unwrap();
        };
        manifest(&apps, "10", "Space Game", "SpaceGame", 4);
        manifest(&apps, "228980", "Steamworks Common Redistributables", "Steamworks Shared", 4);
        manifest(&apps, "30", "Half Downloaded", "Half", 1026);
        manifest(&other.join("steamapps"), "20", "No Program", "NoProgram", 4);
        let game = apps.join("common").join("SpaceGame");
        fs::create_dir_all(game.join("bin")).unwrap();
        fs::write(game.join("unins000.exe"), vec![0; 900]).unwrap();
        fs::write(game.join("Launcher.exe"), vec![0; 500]).unwrap();
        fs::write(game.join("bin").join("SpaceGame.exe"), vec![0; 10]).unwrap();
        let cache = root.join("appcache").join("librarycache").join("10");
        fs::create_dir_all(cache.join("abc")).unwrap();
        fs::write(cache.join("library_600x900.jpg"), b"x").unwrap();
        fs::write(cache.join("abc").join("library_hero.jpg"), b"x").unwrap();

        let found = steam(&root);
        assert_eq!(found.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["No Program", "Space Game"]);
        assert_eq!(found[0].target, "steam://rungameid/20");
        assert_eq!(found[1].target, game.join("bin").join("SpaceGame.exe").to_string_lossy());
        assert_eq!(found[1].art, vec![("cover", cache.join("library_600x900.jpg")), ("banner", cache.join("abc").join("library_hero.jpg"))]);

        let entry = found[1].entry(5);
        assert_eq!((entry.launch_type(), entry.library()), ("path", "games"));
        assert_eq!(entry.0["cwd"], json!(game.join("bin").to_string_lossy()));
        assert!(already_in(&[entry], &found[1]));
        assert_eq!(found[0].entry(5).launch_type(), "url");
    }

    #[test]
    fn main_exe_prefers_the_name_then_the_biggest_nearest_the_top() {
        let dir = temp_dir();
        fs::create_dir_all(dir.path().join("x")).unwrap();
        fs::write(dir.path().join("small.exe"), vec![0; 10]).unwrap();
        fs::write(dir.path().join("x").join("big.exe"), vec![0; 100]).unwrap();
        fs::write(dir.path().join("CrashReporter.exe"), vec![0; 1000]).unwrap();
        assert_eq!(main_exe(dir.path(), &["Nothing Alike"]), Some(dir.path().join("small.exe")));
        assert_eq!(main_exe(dir.path(), &["Big"]), Some(dir.path().join("x").join("big.exe")));
    }

    #[test]
    fn quiver3_reads_apps_json_and_finds_each_installed_apps_program() {
        let dir = temp_dir();
        let q = dir.path();
        let app = q.join("Apps").join("Mario-Recomp");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("unins000.exe"), vec![0; 99]).unwrap();
        fs::write(app.join("Mario64Recomp.exe"), b"x").unwrap();
        fs::write(q.join("QuiverLauncher.exe"), b"x").unwrap();
        let doc = json!({ "apps": [
            { "name": "Mario 64", "folderName": "Mario-Recomp", "installPath": null, "repository": "a/b", "project": "Mario64 Recomp" },
            { "name": "Not Installed", "folderName": "Nope", "installPath": null },
        ]});
        fs::write(q.join("apps.json"), format!("\u{feff}{doc}")).unwrap();
        assert!(is_quiver(q));
        let found = quiver(q);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].key.as_str(), found[0].name.as_str()), ("quiver:github:a/b", "Mario 64"));
        assert_eq!(found[0].target, app.join("Mario64Recomp.exe").to_string_lossy());
    }

    #[test]
    fn quiver_reads_installs_with_their_program_and_name() {
        let dir = temp_dir();
        let data = dir.path();
        let app = data.join("apps").join("Ports").join("Mario64");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("sm64.exe"), b"x").unwrap();
        let chosen = data.join("elsewhere.exe");
        fs::write(&chosen, b"x").unwrap();
        fs::write(data.join("library.json"), json!([
            { "id": "github:a/sm64", "slug": "sm64", "overrides": { "name": "Mario 64" } },
            { "id": "github:b/zelda", "slug": "zelda" },
            { "id": "github:c/gone", "slug": "gone" },
        ]).to_string()).unwrap();
        fs::write(data.join("catalog.json"), json!({ "github:b/zelda": { "name": "Zelda Port" } }).to_string()).unwrap();
        fs::write(data.join("installs.json"), json!({
            "github:a/sm64": { "folder": "Mario64", "group": "Ports", "executables": ["sm64.exe"], "version": "1" },
            "github:b/zelda": { "folder": "Zelda", "launch": { "executable": chosen.to_string_lossy() }, "version": "1" },
            "github:c/gone": { "folder": "Gone", "executables": ["gone.exe"], "version": "1" },
            "github:d/notlisted": { "folder": "X", "version": "1" },
        }).to_string()).unwrap();
        let found = quiver(data);
        assert_eq!(found.iter().map(|f| (f.key.as_str(), f.name.as_str())).collect::<Vec<_>>(), [("quiver:github:a/sm64", "Mario 64"), ("quiver:github:b/zelda", "Zelda Port")]);
        assert_eq!(found[0].target, app.join("sm64.exe").to_string_lossy());
        assert_eq!(found[1].target, chosen.to_string_lossy());
    }
}
