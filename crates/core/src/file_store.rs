//! Stored files: copies of files and folders dropped on the grid, kept in
//! `<data>\files\<id>\<name>` so a tile keeps working after its original is
//! moved or deleted. Thumbnails live in `files\thumbs\<id>.png`. Ported from
//! lib/file-store.js, plus main.js's `files:store` handler (`store_files`).
//!
//! The entries themselves are ordinary apps.json items with type 'stored'
//! (see `store::normalize`, which re-derives their target and icon path
//! from the id on every load rather than trusting the file).
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ids::{is_uuid, new_uuid};
use crate::layout;
use crate::js::js_to_number;
use crate::safe_file::write_file_atomic;
use crate::settings::Settings;
use crate::store::{self, parent_key, strict_eq, Entry, Store};
use crate::{Error, Result};

pub const DEFAULT_LIMIT: f64 = 2.0 * 1024.0 * 1024.0 * 1024.0; // 2 GB
pub const DEFAULT_FILE_LIMIT: f64 = 10_000.0; // files per stored folder
pub const THUMB_SIZE: u32 = 256;

/// Launchable things are never copied: a copied program loses its config and
/// relative paths, and a shortcut's target is what matters. These keep the
/// old behavior of a 'path'/'script' entry pointing at the original.
pub const LINK_EXTENSIONS: [&str; 12] =
    [".exe", ".com", ".msi", ".bat", ".cmd", ".ps1", ".vbs", ".lnk", ".url", ".py", ".js", ".mjs"];

/// Thumbnails and icons from the shell, supplied by the app
/// (IShellItemImageFactory in Phase 2). Both return PNG bytes.
pub trait Thumbnails {
    /// The shell's thumbnail (images, PDFs, videos) at up to `size` px, or
    /// `None` for a file type with no thumbnail handler.
    fn thumbnail(&mut self, path: &Path, size: u32) -> Option<Vec<u8>>;

    /// The file's large icon, `None` if there is none.
    fn file_icon(&mut self, path: &Path) -> Option<Vec<u8>>;
}

/// Why a store failed. `TooLarge` is the JS error with `code: 'tooLarge'`.
#[derive(Debug)]
pub enum StoreError {
    TooLarge { size: u64, files: u64, limit: f64, file_limit: f64 },
    Other(Error),
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Other(e.into())
    }
}

impl From<Error> for StoreError {
    fn from(e: Error) -> Self {
        StoreError::Other(e)
    }
}

pub fn thumbs_dir(dir: &Path) -> PathBuf {
    store::files_dir(dir).join("thumbs")
}

pub fn assert_uuid(id: &str) -> Result<()> {
    if is_uuid(id) {
        Ok(())
    } else {
        Err(Error::msg(format!("Not a stored item id: {id}")))
    }
}

// Node's path.extname: from the last dot of the last component, "" when
// that dot leads the name.
fn extname(p: &Path) -> String {
    p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
}

/// Only files are linked by extension; folders are always copied (subject to
/// the size limit), whatever their name ends in.
pub fn is_linkable(file_path: &str) -> bool {
    LINK_EXTENSIONS.contains(&extname(Path::new(file_path)).to_lowercase().as_str())
}

#[derive(PartialEq)]
enum Kind {
    File,
    Dir,
    Other,
}

// Copied from media::scan: what Node's Dirent says on Windows. Anything with
// a reparse point (a symlink, a junction, a cloud placeholder) is a link, so
// neither a file nor a folder, and loops can't happen.
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

// Total bytes and file count under a folder, skipping symlinks/junctions the
// same way copy_dir does. Stops as soon as either limit is passed, so a huge
// folder is cheap to reject.
fn dir_size(dir: &Path, limit: f64, file_limit: f64, acc: &mut (u64, u64)) -> std::result::Result<(), StoreError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        match kind_of(&entry) {
            Kind::Dir => dir_size(&p, limit, file_limit, acc)?,
            Kind::File => {
                acc.0 += fs::metadata(&p)?.len();
                acc.1 += 1;
            }
            Kind::Other => continue,
        }
        if acc.0 as f64 > limit || acc.1 as f64 > file_limit {
            return Err(StoreError::TooLarge { size: acc.0, files: acc.1, limit, file_limit });
        }
    }
    Ok(())
}

/// Recursive copy. Symlinks and junctions are skipped, so a folder can't loop
/// or pull in content from outside itself.
pub fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (s, d) = (entry.path(), to.join(entry.file_name()));
        match kind_of(&entry) {
            Kind::Dir => copy_dir(&s, &d)?,
            Kind::File => {
                fs::copy(&s, &d)?;
            }
            Kind::Other => {}
        }
    }
    Ok(())
}

fn remove_if_there(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Writes thumbs\<id>.png: the shell's thumbnail where there is one (images,
/// PDFs, videos), else the file's icon. Returns the entry's `thumbState`.
/// Folders get no file: the renderer draws its own folder glyph, since the
/// shell icon of a folder is a drive icon on Windows and the shell has no
/// thumbnail for an empty one.
pub fn make_thumbnail<T: Thumbnails>(
    dir: &Path,
    file_path: &Path,
    id: &str,
    is_folder: bool,
    thumbs: &mut T,
) -> Result<&'static str> {
    let thumb_file = thumbs_dir(dir).join(format!("{id}.png"));
    if is_folder {
        remove_if_there(fs::remove_file(&thumb_file))?;
        return Ok("generic");
    }
    fs::create_dir_all(thumbs_dir(dir))?;
    if let Some(png) = thumbs.thumbnail(file_path, THUMB_SIZE).filter(|b| !b.is_empty()) {
        if write_file_atomic(&thumb_file, png).is_ok() {
            return Ok("ready");
        }
    }
    if let Some(icon) = thumbs.file_icon(file_path).filter(|b| !b.is_empty()) {
        let _ = write_file_atomic(&thumb_file, icon);
    }
    Ok("generic")
}

/// What `store_from_path` made.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub id: String,
    pub stored_name: String,
    pub target: PathBuf,
    /// "file" or "folder".
    pub stored_kind: &'static str,
    pub original_path: PathBuf,
    pub size: u64,
    /// "ready" or "generic".
    pub thumb_state: &'static str,
}

// path.basename, "" for a root (Node also gives ".." for "x\..": not a name
// to copy into).
fn basename(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Copies one file or folder into files\<id>\ under a new id. Never changes
/// the source. On any failure the half-written folder is removed and the
/// error returned.
pub fn store_from_path<T: Thumbnails>(
    dir: &Path,
    source_path: &Path,
    limit: f64,
    file_limit: f64,
    thumbs: &mut T,
) -> std::result::Result<Stored, StoreError> {
    let id = new_uuid();
    let src = fs::metadata(source_path)?;
    let is_folder = src.is_dir();
    if !src.is_file() && !is_folder {
        return Err(Error::msg("Not a file or folder").into());
    }

    let size = if is_folder {
        let mut acc = (0, 0);
        dir_size(source_path, limit, file_limit, &mut acc)?;
        acc.0
    } else {
        src.len()
    };
    if size as f64 > limit {
        return Err(StoreError::TooLarge { size, files: 1, limit, file_limit });
    }

    let name = basename(source_path);
    let dest = store::files_dir(dir).join(&id);
    let final_path = dest.join(&name);
    let copied = fs::create_dir_all(&dest).and_then(|()| {
        if is_folder {
            copy_dir(source_path, &final_path)
        } else {
            fs::copy(source_path, &final_path).map(|_| ())
        }
    });
    if let Err(e) = copied {
        let _ = fs::remove_dir_all(&dest);
        return Err(e.into());
    }

    let thumb_state = make_thumbnail(dir, &final_path, &id, is_folder, thumbs)?;
    Ok(Stored {
        id,
        stored_name: name,
        target: final_path,
        stored_kind: if is_folder { "folder" } else { "file" },
        original_path: source_path.to_owned(),
        size,
        thumb_state,
    })
}

/// Deletes a stored item's copy and thumbnail. Only ever touches
/// files\<uuid>\ and thumbs\<uuid>.png - a bad id must never become a delete
/// somewhere else.
pub fn remove_stored(dir: &Path, id: &str) -> Result<()> {
    assert_uuid(id)?;
    remove_if_there(fs::remove_dir_all(store::files_dir(dir).join(id)))?;
    remove_if_there(fs::remove_file(thumbs_dir(dir).join(format!("{id}.png"))))?;
    Ok(())
}

// `String(a.name)` for the names a hand-edited apps.json might hold.
fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Display name for a new stored item: the file name without its extension
/// (folders keep theirs), with " (2)", " (3)"... when a sibling already uses it.
fn display_name(apps: &[Entry], parent_id: Option<&Value>, stored: &Stored) -> String {
    let name = &stored.stored_name;
    let ext = if stored.stored_kind == "file" { extname(Path::new(name)) } else { String::new() };
    let base = match &name[..name.len() - ext.len()] {
        "" => name.as_str(),
        b => b,
    };
    let taken: Vec<String> =
        apps.iter().filter(|a| strict_eq(parent_key(a), parent_id)).map(|a| js_string(a.get("name")).to_lowercase()).collect();
    let free = |n: &str| !taken.contains(&n.to_lowercase());
    if free(base) {
        return base.to_owned();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|c| free(c)).expect("some number is free")
}

fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn new_entry(dir: &Path, apps: &[Entry], stored: &Stored, opts: &StoreOptions) -> Entry {
    let parent = opts.parent_id.as_deref().filter(|p| !p.is_empty()).map(Value::from);
    let occupied = layout::sibling_footprints(apps, parent.as_ref(), Some(&Value::from(stored.id.as_str())));
    let pos = layout::find_nearest_free_cell(&occupied, opts.grid_x, opts.grid_y, 1.0, 1.0);
    let mut e = Entry::new();
    e.set("id", stored.id.as_str());
    e.set("kind", "app");
    e.set("name", display_name(apps, parent.as_ref(), stored));
    e.set("type", "stored");
    e.set("target", path_text(&stored.target));
    e.set("storedName", stored.stored_name.as_str());
    e.set("storedKind", stored.stored_kind);
    e.set("originalPath", path_text(&stored.original_path));
    e.set_num("size", stored.size as f64);
    e.set("thumbState", stored.thumb_state);
    for key in ["args", "cwd", "interpreter", "command"] {
        e.set(key, "");
    }
    e.set("parentId", parent.unwrap_or(Value::Null));
    e.set("pinned", false);
    e.set("createdAt", opts.now);
    e.set("lastLaunched", Value::Null);
    e.set("launchCount", 0);
    e.set("color", Value::Null);
    e.set("iconIsCustom", false);
    e.set_num("gridX", pos.x);
    e.set_num("gridY", pos.y);
    store::normalize(dir, &mut e);
    e
}

/// `0` B to TB, as the size-limit message shows it.
pub fn format_size(bytes: f64) -> String {
    if bytes.is_nan() || bytes < 0.0 {
        return String::new();
    }
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let (mut n, mut i) = (bytes, 0);
    while n >= 1024.0 && i < UNITS.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        return if n == 1.0 { "1 byte".into() } else { format!("{n} bytes") };
    }
    // toFixed sends a half up; Rust's {:.1} would send it to even.
    let digits = if n < 10.0 { 1 } else { 0 };
    let scale = 10f64.powi(digits);
    format!("{:.*} {}", digits as usize, crate::js::js_round(n * scale) / scale, UNITS[i])
}

// 10000 -> "10,000", as en-US toLocaleString writes it.
fn grouped(n: f64) -> String {
    let digits = format!("{n}");
    let (int, frac) = digits.split_once('.').map_or((digits.as_str(), None), |(i, f)| (i, Some(f)));
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f[..f.len().min(3)]);
    }
    out
}

/// The message shown for a path that couldn't be stored.
pub fn friendly_error(err: &StoreError, source_path: &Path) -> String {
    let name = match basename(source_path) {
        n if n.is_empty() => path_text(source_path),
        n => n,
    };
    let io = match err {
        StoreError::TooLarge { limit, file_limit, .. } => {
            return format!(
                "\"{name}\" is larger than the file store limit ({} or {} files).",
                format_size(*limit),
                grouped(*file_limit)
            );
        }
        StoreError::Other(Error::Io(e)) => Some(e),
        StoreError::Other(_) => None,
    };
    // libuv's names: ENOENT, ENOSPC (ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL),
    // EACCES/EPERM, EBUSY (sharing and lock violations).
    if let Some(e) = io {
        if e.kind() == io::ErrorKind::NotFound {
            return format!("\"{name}\" could not be read. It may have been moved or deleted.");
        }
        if matches!(e.raw_os_error(), Some(112 | 39)) || e.kind() == io::ErrorKind::StorageFull {
            return format!("There isn't enough disk space to store \"{name}\".");
        }
        if e.kind() == io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33)) {
            return format!("\"{name}\" could not be read. It may be open in another program or need permission.");
        }
    }
    let message = match err {
        StoreError::Other(e) => e.to_string(),
        StoreError::TooLarge { .. } => unreachable!(),
    };
    format!("\"{name}\" could not be stored: {message}")
}

/// `storeMany`'s options.
#[derive(Debug, Clone)]
pub struct StoreOptions {
    /// The island to put the items in, None for Home.
    pub parent_id: Option<String>,
    pub grid_x: f64,
    pub grid_y: f64,
    pub limit: f64,
    pub file_limit: f64,
    /// createdAt for the new entries.
    pub now: i64,
}

impl Default for StoreOptions {
    fn default() -> Self {
        StoreOptions {
            parent_id: None,
            grid_x: 0.0,
            grid_y: 0.0,
            limit: DEFAULT_LIMIT,
            file_limit: DEFAULT_FILE_LIMIT,
            now: crate::time::now_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StoreResult {
    Ok { path: PathBuf, id: String },
    Failed { path: PathBuf, too_large: bool, message: String },
}

impl StoreResult {
    pub fn ok(&self) -> bool {
        matches!(self, StoreResult::Ok { .. })
    }
}

/// Stores each path in turn and appends an entry for each one that worked to
/// `apps`. A failure on one path is reported in the results and doesn't stop
/// the others; nothing is added for a failed copy.
pub fn store_many<T: Thumbnails>(
    dir: &Path,
    apps: &mut Vec<Entry>,
    paths: &[PathBuf],
    opts: &StoreOptions,
    thumbs: &mut T,
) -> Vec<StoreResult> {
    let mut results = Vec::new();
    for p in paths {
        match store_from_path(dir, p, opts.limit, opts.file_limit, thumbs) {
            Ok(stored) => {
                let entry = new_entry(dir, apps, &stored, opts);
                apps.push(entry);
                results.push(StoreResult::Ok { path: p.clone(), id: stored.id });
            }
            Err(err) => results.push(StoreResult::Failed {
                path: p.clone(),
                too_large: matches!(err, StoreError::TooLarge { .. }),
                message: friendly_error(&err, p),
            }),
        }
    }
    results
}

/// main.js's fileStoreOptions: the byte limit from settings.json and what a
/// plain drop does ("copy" or "link").
pub fn file_store_options(s: &Settings) -> (f64, &'static str) {
    let limit = js_to_number(s.get("fileStoreLimit"));
    let limit = if limit.is_finite() && limit > 0.0 { limit } else { DEFAULT_LIMIT };
    let on_drop = if s.get("fileStoreOnDrop").and_then(Value::as_str) == Some("link") { "link" } else { "copy" };
    (limit, on_drop)
}

/// main.js's `files:store`: copies `paths` in as stored entries of the
/// island `parent_id` (Home if it isn't one) near (grid_x, grid_y), saves
/// apps.json, and returns the new list and a result per path.
pub fn store_files<T: Thumbnails>(
    store: &mut Store,
    paths: &[String],
    parent_id: Option<&str>,
    grid_x: f64,
    grid_y: f64,
    thumbs: &mut T,
) -> Result<(Vec<Entry>, Vec<StoreResult>)> {
    let list: Vec<PathBuf> = paths.iter().filter(|p| !p.is_empty()).map(PathBuf::from).collect();
    let mut apps = store.load()?;
    // Refuse before copying anything: a copy with no entry pointing at it
    // would just be an orphan in the store.
    if store.is_read_only() {
        store.save(&apps)?; // fails with the read-only message
    }
    let parent = parent_id
        .filter(|p| !p.is_empty() && apps.iter().any(|a| a.str("id") == Some(p) && a.str("kind") == Some("island")));
    let whole = |n: f64| if n.is_finite() && n.fract() == 0.0 { n } else { 0.0 };
    let dir = store.data_dir().to_owned();
    let opts = StoreOptions {
        parent_id: parent.map(str::to_owned),
        grid_x: whole(grid_x),
        grid_y: whole(grid_y),
        limit: file_store_options(&crate::settings::load(&dir)).0,
        ..StoreOptions::default()
    };
    let results = store_many(&dir, &mut apps, &list, &opts, thumbs);
    let added: Vec<&str> = results
        .iter()
        .filter_map(|r| match r {
            StoreResult::Ok { id, .. } => Some(id.as_str()),
            StoreResult::Failed { .. } => None,
        })
        .collect();
    if !added.is_empty() {
        if let Err(err) = store.save(&apps) {
            for id in added {
                let _ = remove_stored(&dir, id);
            }
            return Err(err);
        }
    }
    Ok((apps, results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{list_dir, temp_dir, TempDir};
    use serde_json::json;

    // Shell stand-in: anything ending in an image extension "has" a
    // thumbnail, everything else has none, like the real shell call for a
    // file type with no thumbnail handler.
    struct FakeShell {
        icon: bool,
    }

    impl Thumbnails for FakeShell {
        fn thumbnail(&mut self, path: &Path, _: u32) -> Option<Vec<u8>> {
            let ext = extname(path).to_lowercase();
            [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"].contains(&ext.as_str()).then(|| b"thumb".to_vec())
        }
        fn file_icon(&mut self, _: &Path) -> Option<Vec<u8>> {
            self.icon.then(|| b"icon".to_vec())
        }
    }

    fn shell() -> FakeShell {
        FakeShell { icon: true }
    }

    struct Setup {
        dir: TempDir,
        src: PathBuf,
        files_dir: PathBuf,
    }

    fn setup() -> Setup {
        let dir = temp_dir();
        let src = dir.path().join("source");
        fs::create_dir(&src).unwrap();
        let files_dir = dir.path().join("files");
        Setup { dir, src, files_dir }
    }

    fn write(file: PathBuf, data: &str) -> PathBuf {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, data).unwrap();
        file
    }

    fn store_default(t: &Setup, p: &Path) -> std::result::Result<Stored, StoreError> {
        store_from_path(t.dir.path(), p, DEFAULT_LIMIT, DEFAULT_FILE_LIMIT, &mut shell())
    }

    #[test]
    fn copying_a_file_keeps_its_bytes_and_leaves_the_original_alone() {
        let t = setup();
        let original = write(t.src.join("report.pdf"), "pdf bytes");
        let before = fs::metadata(&original).unwrap().modified().unwrap();

        let stored = store_default(&t, &original).unwrap();
        assert_eq!(stored.target, t.files_dir.join(&stored.id).join("report.pdf"));
        assert_eq!(fs::read_to_string(&stored.target).unwrap(), "pdf bytes");
        assert_eq!(stored.stored_kind, "file");
        assert_eq!(stored.size, 9);
        assert_eq!(stored.original_path, original);
        assert_eq!(fs::read_to_string(&original).unwrap(), "pdf bytes");
        assert_eq!(fs::metadata(&original).unwrap().modified().unwrap(), before);

        // Still opens after the original is deleted.
        fs::remove_file(&original).unwrap();
        assert!(stored.target.exists());
    }

    // A junction, which needs no admin rights, unlike a symlink.
    fn junction(link: &Path, target: &Path) {
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J failed");
    }

    #[cfg(windows)]
    #[test]
    fn copying_a_folder_brings_nested_files_and_skips_links_out_of_it() {
        let t = setup();
        let folder = t.src.join("Photos");
        write(folder.join("a.txt"), "a");
        write(folder.join("nested").join("deeper").join("b.txt"), "bb");
        let outside = t.dir.path().join("outside");
        write(outside.join("secret.txt"), "nope");
        junction(&folder.join("link"), &outside);

        let stored = store_default(&t, &folder).unwrap();
        assert_eq!(stored.stored_kind, "folder");
        assert_eq!(stored.size, 3, "the junction is not counted either");
        assert_eq!(list_dir(&stored.target), ["a.txt", "nested"]);
        assert_eq!(fs::read_to_string(stored.target.join("nested").join("deeper").join("b.txt")).unwrap(), "bb");
    }

    #[test]
    fn a_folder_over_the_size_or_file_count_limit_is_rejected_and_nothing_is_written() {
        let t = setup();
        let folder = t.src.join("Big");
        for i in 0..5 {
            write(folder.join(format!("{i}.bin")), "0123456789");
        }
        let too_large = |r: std::result::Result<Stored, StoreError>| matches!(r, Err(StoreError::TooLarge { .. }));
        let d = t.dir.path();
        assert!(too_large(store_from_path(d, &folder, 25.0, DEFAULT_FILE_LIMIT, &mut shell())));
        assert!(too_large(store_from_path(d, &folder, DEFAULT_LIMIT, 3.0, &mut shell())));
        assert!(too_large(store_from_path(d, &folder.join("0.bin"), 5.0, DEFAULT_FILE_LIMIT, &mut shell())));
        assert!(!t.files_dir.exists() || list_dir(&t.files_dir).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_copy_that_fails_part_way_removes_its_half_written_folder() {
        use std::os::windows::fs::OpenOptionsExt;
        let t = setup();
        let folder = t.src.join("Docs");
        write(folder.join("one.txt"), "1");
        let two = write(folder.join("two.txt"), "2");

        // JS fakes copyFile failing on its second call; here the second file
        // is held open with no sharing, so copying it fails for real.
        let lock = fs::OpenOptions::new().read(true).share_mode(0).open(&two).unwrap();
        let err = store_default(&t, &folder).unwrap_err();
        drop(lock);
        assert!(matches!(err, StoreError::Other(Error::Io(_))), "{err:?}");
        assert!(friendly_error(&err, &folder).contains("may be open in another program"));
        assert_eq!(list_dir(&t.files_dir), Vec::<String>::new());
        assert_eq!(list_dir(&folder), ["one.txt", "two.txt"], "source untouched");
    }

    #[test]
    fn remove_stored_only_deletes_inside_the_store_and_only_for_uuid_ids() {
        let t = setup();
        let stored = store_default(&t, &write(t.src.join("pic.png"), "png")).unwrap();
        let keep = write(t.dir.path().join("keep.txt"), "keep");

        // JS also tries null; a &str can't be null.
        let sneaky = format!("{}\\..\\..", stored.id);
        for bad in ["..", "../keep.txt", "..\\source", "", "thumbs", &sneaky] {
            let err = remove_stored(t.dir.path(), bad).unwrap_err().to_string();
            assert!(err.contains("Not a stored item id"), "{bad}");
        }
        assert!(keep.exists());
        assert!(t.src.join("pic.png").exists());
        assert!(stored.target.exists());

        remove_stored(t.dir.path(), &stored.id).unwrap();
        assert!(!t.files_dir.join(&stored.id).exists());
        assert!(!t.files_dir.join("thumbs").join(format!("{}.png", stored.id)).exists());
        assert!(t.src.join("pic.png").exists(), "the original is never touched");
    }

    #[test]
    fn thumbnails_images_are_ready_folders_generic_other_files_fall_back_to_the_shell_icon() {
        let t = setup();
        let thumb = |id: &str| t.files_dir.join("thumbs").join(format!("{id}.png"));

        let img = store_default(&t, &write(t.src.join("cat.jpg"), "jpg")).unwrap();
        assert_eq!(img.thumb_state, "ready");
        assert_eq!(fs::read_to_string(thumb(&img.id)).unwrap(), "thumb");

        let doc = store_default(&t, &write(t.src.join("notes.xyz"), "x")).unwrap();
        assert_eq!(doc.thumb_state, "generic");
        assert_eq!(fs::read_to_string(thumb(&doc.id)).unwrap(), "icon");

        fs::create_dir(t.src.join("Folder")).unwrap();
        let folder = store_default(&t, &t.src.join("Folder")).unwrap();
        assert_eq!(folder.thumb_state, "generic");
        assert!(!thumb(&folder.id).exists(), "folders use the renderer glyph");

        // No icon either: still generic, and no file written.
        let bare_src = write(t.src.join("bare.qqq"), "q");
        let bare = store_from_path(t.dir.path(), &bare_src, DEFAULT_LIMIT, DEFAULT_FILE_LIMIT, &mut FakeShell { icon: false })
            .unwrap();
        assert_eq!(bare.thumb_state, "generic");
        assert!(!thumb(&bare.id).exists());
    }

    fn app(v: Value) -> Entry {
        Entry::from_value(v).unwrap()
    }

    #[test]
    fn store_many_with_one_good_and_one_bad_path_adds_one_entry_and_reports_one_failure() {
        let t = setup();
        let good = write(t.src.join("song.mp3"), "mp3");
        let bad = t.src.join("gone.txt");
        let mut apps = vec![app(json!({ "id": store::new_id(), "kind": "app", "name": "Existing", "parentId": null, "gridX": 2, "gridY": 1 }))];

        let opts = StoreOptions { grid_x: 2.0, grid_y: 1.0, ..StoreOptions::default() };
        let results = store_many(t.dir.path(), &mut apps, &[bad, good], &opts, &mut shell());
        assert_eq!(results.iter().map(StoreResult::ok).collect::<Vec<_>>(), [false, true]);
        let StoreResult::Failed { message, .. } = &results[0] else { unreachable!() };
        assert!(message.contains("\"gone.txt\" could not be read"), "{message}");
        assert_eq!(apps.len(), 2);

        let entry = &apps[1];
        assert_eq!(entry.str("type"), Some("stored"));
        assert_eq!(entry.str("name"), Some("song"));
        let StoreResult::Ok { id, .. } = &results[1] else { unreachable!() };
        assert_eq!(entry.str("id"), Some(id.as_str()));
        assert_ne!((entry.num("gridX"), entry.num("gridY")), (Some(2.0), Some(1.0)), "lands next to the occupied cell");
        assert!(Path::new(entry.str("target").unwrap()).exists());
    }

    #[test]
    fn a_second_drop_of_the_same_file_is_a_second_copy_named_2() {
        let t = setup();
        let file = write(t.src.join("photo.png"), "png");
        let mut apps = vec![];
        store_many(t.dir.path(), &mut apps, &[file.clone(), file], &StoreOptions::default(), &mut shell());
        assert_eq!(apps.iter().map(|a| a.str("name").unwrap()).collect::<Vec<_>>(), ["photo", "photo (2)"]);
        assert_ne!(apps[0].str("target"), apps[1].str("target"));
        assert!(apps.iter().all(|a| Path::new(a.str("target").unwrap()).exists()));
    }

    #[test]
    fn store_many_puts_items_in_the_given_island() {
        let t = setup();
        let mut apps = vec![];
        let opts = StoreOptions { parent_id: Some("island-1".into()), ..StoreOptions::default() };
        store_many(t.dir.path(), &mut apps, &[write(t.src.join("a.txt"), "a")], &opts, &mut shell());
        assert_eq!(apps[0].str("parentId"), Some("island-1"));
    }

    #[test]
    fn a_stored_entrys_target_is_re_derived_on_load_never_trusted_from_apps_json() {
        let t = setup();
        let id = store::new_id();
        let base = |extra: Value| {
            let mut e = json!({ "id": id, "kind": "app", "name": "x", "type": "stored", "createdAt": 1 });
            e.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            e
        };
        let list = json!([
            base(json!({ "storedName": "x.txt", "target": "C:\\Windows\\System32\\cmd.exe", "iconPath": "C:\\evil.png" })),
            base(json!({ "id": store::new_id(), "storedName": "..\\..\\..\\Windows\\notepad.exe" })),
            base(json!({ "id": "../../escape", "storedName": "x.txt" })),
        ]);
        fs::write(t.dir.path().join("apps.json"), list.to_string()).unwrap();
        let loaded = Store::new(t.dir.path()).load().unwrap();
        let [ok, bad_name, bad_id] = &loaded[..] else { panic!("{loaded:?}") };
        assert_eq!(ok.str("target").map(PathBuf::from), Some(t.files_dir.join(&id).join("x.txt")));
        assert_eq!(ok.str("iconPath").map(PathBuf::from), Some(t.files_dir.join("thumbs").join(format!("{id}.png"))));
        assert_eq!(ok.str("thumbState"), Some("pending"));
        assert_eq!(bad_name.str("target"), Some(""));
        assert_eq!(bad_id.str("target"), Some(""));
        assert_eq!(bad_id.get("iconPath"), Some(&Value::Null));
    }

    #[test]
    fn programs_scripts_and_shortcuts_are_links_everything_else_is_copied() {
        for p in ["C:\\a\\app.EXE", "x.lnk", "x.url", "setup.msi", "run.bat", "tool.py", "a.mjs", "x.ps1"] {
            assert!(is_linkable(p), "{p}");
        }
        for p in ["photo.png", "report.pdf", "archive.zip", "C:\\Users\\me\\Folder", "C:\\dir.exe\\file"] {
            assert!(!is_linkable(p), "{p}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn files_under_a_path_longer_than_260_characters_copy() {
        let t = setup();
        let mut deep = t.src.join("Long");
        while deep.as_os_str().len() < 300 {
            deep = deep.join("a-fairly-long-folder-name-for-testing");
        }
        write(deep.join("deep.txt"), "deep");

        let stored = store_default(&t, &t.src.join("Long")).unwrap();
        let copied = stored.target.join(deep.strip_prefix(t.src.join("Long")).unwrap()).join("deep.txt");
        assert!(copied.as_os_str().len() > 260);
        assert_eq!(fs::read_to_string(&copied).unwrap(), "deep");
        assert_eq!(stored.size, 4);
    }

    // Not in the JS tests: what the messages say (checked against node), and
    // main.js's files:store around store_many.
    #[test]
    fn friendly_errors_and_sizes_read_as_in_the_js() {
        let big = StoreError::TooLarge { size: 0, files: 0, limit: DEFAULT_LIMIT, file_limit: DEFAULT_FILE_LIMIT };
        assert_eq!(
            friendly_error(&big, Path::new("C:\\x\\Big")),
            "\"Big\" is larger than the file store limit (2.0 GB or 10,000 files)."
        );
        let gone = StoreError::Other(io::Error::from(io::ErrorKind::NotFound).into());
        assert_eq!(friendly_error(&gone, Path::new("C:\\")), "\"C:\\\" could not be read. It may have been moved or deleted.");
        let full = StoreError::Other(io::Error::from_raw_os_error(112).into());
        assert_eq!(friendly_error(&full, Path::new("a.txt")), "There isn't enough disk space to store \"a.txt\".");
        let other = StoreError::Other(Error::msg("Not a file or folder"));
        assert_eq!(friendly_error(&other, Path::new("a")), "\"a\" could not be stored: Not a file or folder");
        let sizes: Vec<String> = [-1.0, 0.0, 1.0, 1023.0, 1024.0, 1075.2, 1126.4, 10239.0, 1.5 * 1024.0 * 1024.0, 5e12]
            .iter()
            .map(|b| format_size(*b))
            .collect();
        assert_eq!(sizes, ["", "0 bytes", "1 byte", "1023 bytes", "1.0 KB", "1.1 KB", "1.1 KB", "10.0 KB", "1.5 MB", "4.5 TB"]);
    }

    #[test]
    fn store_files_refuses_while_read_only_and_puts_items_only_in_real_islands() {
        let t = setup();
        let file = write(t.src.join("a.txt"), "a").to_string_lossy().into_owned();
        fs::write(t.dir.path().join("apps.json"), "{ not json").unwrap();
        let mut store = Store::new(t.dir.path());
        let err = store_files(&mut store, std::slice::from_ref(&file), None, 0.0, 0.0, &mut shell()).unwrap_err();
        assert_eq!(err.to_string(), store::READ_ONLY_MESSAGE);
        assert!(!t.files_dir.exists() || list_dir(&t.files_dir).is_empty(), "nothing copied");

        let island = store::new_id();
        let apps = json!([{ "id": island, "kind": "island", "name": "I" }]);
        fs::write(t.dir.path().join("apps.json"), apps.to_string()).unwrap();
        let (apps, results) =
            store_files(&mut store, &[file.clone(), String::new()], Some(&island), 0.5, 0.0, &mut shell()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(apps[1].str("parentId"), Some(island.as_str()));
        let (apps, _) = store_files(&mut store, &[file], Some("not-an-island"), 0.0, 0.0, &mut shell()).unwrap();
        assert_eq!(apps[2].get("parentId"), Some(&Value::Null));
        assert_eq!(Store::new(t.dir.path()).load().unwrap().len(), 3, "saved");
    }
}
