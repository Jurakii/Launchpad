//! The renderer's visual preferences (theme, accent, wallpaper, bar layout,
//! tray order, Media colours). 1.2.2 kept them in Chromium's localStorage;
//! 1.3 keeps them in `ui.json`, an object of the same key names and string
//! values. The one change: `launchpad-wallpaper` held a data URL, and now
//! names a file in the data folder (`wallpaper.png`, ...).
//!
//! [`migrate`] runs on 1.3's first start: it reads Electron's leveldb once,
//! writes `ui.json`, then deletes the Chromium folders Electron left behind.
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::safe_file::write_file_atomic;

pub const UI_FILE: &str = "ui.json";
pub const WALLPAPER_KEY: &str = "launchpad-wallpaper";

/// What Chromium and Electron create in the data folder (listed from a real
/// 1.2.2 profile, plus names other Chromium versions use). Never anything
/// Launchpad wrote itself.
pub const CHROMIUM_LEFTOVERS: &[&str] = &[
    "blob_storage",
    "Cache",
    "Code Cache",
    "Crashpad",
    "databases",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "declarative_performance_observer.db",
    "declarative_performance_observer.db-journal",
    "DevToolsActivePort",
    "Dictionaries",
    "DIPS",
    "DIPS-journal",
    "GPUCache",
    "IndexedDB",
    "Local State",
    "Local Storage",
    "Network",
    "Partitions",
    "Preferences",
    "Service Worker",
    "Session Storage",
    "Shared Dictionary",
    "SharedStorage",
    "SharedStorage-wal",
    "Trust Tokens",
    "Trust Tokens-journal",
    "WebStorage",
    // electron-updater's install id; 1.3 has its own updater.
    ".updaterId",
];

/// `ui.json`: key → value, in the order they were written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiPrefs(Map<String, Value>);

impl UiPrefs {
    /// A missing or unreadable `ui.json` is empty prefs: default looks.
    pub fn load(dir: &Path) -> Self {
        let map: Map<String, Value> = fs::read(dir.join(UI_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        UiPrefs(map.into_iter().filter(|(_, v)| v.is_string()).collect())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.0.insert(key.to_owned(), Value::String(value.into()));
    }

    pub fn remove(&mut self, key: &str) {
        self.0.shift_remove(key);
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.0).expect("strings always serialize");
        write_file_atomic(&dir.join(UI_FILE), json)
    }
}

/// What [`migrate`] did, for the log.
#[derive(Debug, Default, PartialEq)]
pub struct Migrated {
    /// `ui.json` was already there, so nothing was done.
    pub already_done: bool,
    /// Keys read from localStorage and written to `ui.json`.
    pub keys: Vec<String>,
    /// The wallpaper file written, e.g. `wallpaper.png`.
    pub wallpaper: Option<String>,
    /// Why localStorage could not be read (1.3 then starts with default looks).
    pub read_error: Option<String>,
    /// Names removed from the data folder.
    pub deleted: Vec<String>,
}

/// First-run move from Chromium's localStorage to `ui.json`. Only failing to
/// write `ui.json` is an error, and then nothing is deleted.
pub fn migrate(dir: &Path) -> crate::Result<Migrated> {
    let mut out = Migrated::default();
    if dir.join(UI_FILE).exists() {
        out.already_done = true;
        return Ok(out);
    }
    let mut prefs = UiPrefs::default();
    match read_local_storage(&dir.join("Local Storage").join("leveldb")) {
        Ok(pairs) => {
            for (k, v) in pairs {
                prefs.set(&k, v);
            }
        }
        Err(e) => out.read_error = Some(e),
    }
    if let Some(url) = prefs.get(WALLPAPER_KEY).map(str::to_owned) {
        prefs.remove(WALLPAPER_KEY);
        if let Some((ext, bytes)) = decode_image_data_url(&url) {
            let name = format!("wallpaper.{ext}");
            if write_file_atomic(&dir.join(&name), bytes).is_ok() {
                prefs.set(WALLPAPER_KEY, name.clone());
                out.wallpaper = Some(name);
            }
        }
    }
    out.keys = prefs.keys().map(str::to_owned).collect();
    prefs.save(dir)?;

    for name in CHROMIUM_LEFTOVERS {
        // Unreadable: keep it, so the 1.2.2 looks aren't lost for good.
        if out.read_error.is_some() && *name == "Local Storage" {
            continue;
        }
        let p = dir.join(name);
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        let gone = if meta.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
        if gone.is_ok() {
            out.deleted.push((*name).to_owned());
        }
    }
    Ok(out)
}

/// `data:image/png;base64,...` → ("png", bytes). None for anything else.
fn decode_image_data_url(url: &str) -> Option<(&'static str, Vec<u8>)> {
    let (head, data) = url.strip_prefix("data:")?.split_once(',')?;
    let mime = head.strip_suffix(";base64")?.to_ascii_lowercase();
    let ext = match mime.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => return None,
    };
    Some((ext, base64_decode(data)?))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().take_while(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// The `launchpad-*` keys of the `file://` origin, newest values, sorted by
/// key. Chromium's layout: key `_file://\0` + encoded name, value = encoded
/// text, where "encoded" is a byte (0 = UTF-16LE, 1 = Latin-1) then the data.
pub fn read_local_storage(leveldb_dir: &Path) -> Result<Vec<(String, String)>, String> {
    const PREFIX: &[u8] = b"_file://\0";
    let mut pairs: Vec<(String, String)> = leveldb::read_all(leveldb_dir)?
        .into_iter()
        .filter_map(|(k, v)| {
            let name = decode_text(k.strip_prefix(PREFIX)?)?;
            name.starts_with("launchpad-").then_some((name, decode_text(&v)?))
        })
        .collect();
    pairs.sort();
    Ok(pairs)
}

fn decode_text(b: &[u8]) -> Option<String> {
    match b.split_first()? {
        (0, rest) => Some(String::from_utf16_lossy(
            &rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>(),
        )),
        (1, rest) => Some(rest.iter().map(|&c| c as char).collect()),
        _ => None,
    }
}

/// Just enough of LevelDB to read a closed database: the write-ahead logs,
/// the sorted tables, and the MANIFEST that says which of them are live.
mod leveldb {
    use super::*;

    /// Called with (internal key, value) for each table entry.
    type Visit<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), String> + 'a;

    const BLOCK: usize = 32 * 1024;
    const TABLE_MAGIC: u64 = 0xdb47_7524_8b80_fb57;

    /// Every live key with its newest value; deleted keys are left out.
    pub fn read_all(dir: &Path) -> Result<HashMap<Vec<u8>, Vec<u8>>, String> {
        let (tables, logs) = live_files(dir)?;
        // key -> (sequence, value or None for a deletion)
        let mut latest: HashMap<Vec<u8>, (u64, Option<Vec<u8>>)> = HashMap::new();
        let mut put = |key: &[u8], seq: u64, value: Option<&[u8]>| {
            if latest.get(key).is_none_or(|(s, _)| seq > *s) {
                latest.insert(key.to_vec(), (seq, value.map(<[u8]>::to_vec)));
            }
        };
        for n in tables {
            let path = dir.join(format!("{n:06}.ldb"));
            let path = if path.exists() { path } else { dir.join(format!("{n:06}.sst")) };
            let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            read_table(&data, &mut |ikey, value| {
                if ikey.len() < 8 {
                    return Err("short internal key".into());
                }
                let (key, trailer) = ikey.split_at(ikey.len() - 8);
                let t = u64::from_le_bytes(trailer.try_into().unwrap());
                put(key, t >> 8, (t & 0xff == 1).then_some(value));
                Ok(())
            })
            .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        for n in logs {
            let path = dir.join(format!("{n:06}.log"));
            let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            for batch in log_records(&data) {
                // A batch that doesn't parse is the torn tail; skip it.
                let _ = read_batch(&batch, &mut put);
            }
        }
        Ok(latest.into_iter().filter_map(|(k, (_, v))| Some((k, v?))).collect())
    }

    /// (table numbers, log numbers) from CURRENT → MANIFEST. Falls back to
    /// every `.ldb`/`.log` in the folder if the MANIFEST can't be read.
    fn live_files(dir: &Path) -> Result<(Vec<u64>, Vec<u64>), String> {
        if let Some(live) = from_manifest(dir) {
            return Ok(live);
        }
        let (mut tables, mut logs) = (vec![], vec![]);
        for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((num, ext)) = name.split_once('.') else { continue };
            let Ok(n) = num.parse::<u64>() else { continue };
            match ext {
                "ldb" | "sst" => tables.push(n),
                "log" => logs.push(n),
                _ => {}
            }
        }
        logs.sort();
        Ok((tables, logs))
    }

    fn from_manifest(dir: &Path) -> Option<(Vec<u64>, Vec<u64>)> {
        let current = fs::read_to_string(dir.join("CURRENT")).ok()?;
        let data = fs::read(dir.join(current.trim())).ok()?;
        let mut tables: Vec<u64> = vec![];
        let (mut log_number, mut prev_log) = (0u64, 0u64);
        for edit in log_records(&data) {
            let mut r = Reader(&edit);
            while !r.0.is_empty() {
                match r.varint()? {
                    1 => drop(r.slice()?),        // comparator
                    2 => log_number = r.varint()?,
                    3 | 4 => drop(r.varint()?),   // next file, last sequence
                    5 => drop((r.varint()?, r.slice()?)), // compact pointer
                    6 => {
                        let (_level, n) = (r.varint()?, r.varint()?);
                        tables.retain(|&t| t != n);
                    }
                    7 => {
                        let (_level, n, _size) = (r.varint()?, r.varint()?, r.varint()?);
                        r.slice()?;
                        r.slice()?;
                        tables.push(n);
                    }
                    9 => prev_log = r.varint()?,
                    _ => return None,
                }
            }
        }
        // Logs at or after the MANIFEST's log number haven't been made into tables yet.
        let mut logs: Vec<u64> = fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".log")?.parse().ok())
            .filter(|&n| n >= log_number || (prev_log != 0 && n == prev_log))
            .collect();
        logs.sort();
        Some((tables, logs))
    }

    /// The records of a log-format file (the WAL and the MANIFEST). Stops at
    /// the first torn or corrupt record: what follows it is a killed write.
    pub fn log_records(data: &[u8]) -> Vec<Vec<u8>> {
        let mut out = vec![];
        let mut partial: Option<Vec<u8>> = None;
        let mut pos = 0;
        while pos + 7 <= data.len() {
            let left_in_block = BLOCK - pos % BLOCK;
            if left_in_block < 7 {
                pos += left_in_block;
                continue;
            }
            let crc = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            let len = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
            let kind = data[pos + 6];
            let end = pos + 7 + len;
            if end > data.len() || 7 + len > left_in_block {
                break;
            }
            if kind == 0 && len == 0 {
                // Zero padding at the end of a block.
                pos += left_in_block;
                continue;
            }
            let body = &data[pos + 7..end];
            if unmask(crc) != crc32c(&data[pos + 6..end]) {
                break;
            }
            pos = end;
            match kind {
                1 => out.push(body.to_vec()),
                2 => partial = Some(body.to_vec()),
                3 => match partial.as_mut() {
                    Some(p) => p.extend_from_slice(body),
                    None => continue,
                },
                4 => {
                    if let Some(mut p) = partial.take() {
                        p.extend_from_slice(body);
                        out.push(p);
                    }
                }
                _ => break,
            }
        }
        out
    }

    /// A WriteBatch: sequence (u64), count (u32), then Put/Delete records.
    fn read_batch(b: &[u8], put: &mut impl FnMut(&[u8], u64, Option<&[u8]>)) -> Option<()> {
        if b.len() < 12 {
            return None;
        }
        let seq = u64::from_le_bytes(b[..8].try_into().unwrap());
        let count = u32::from_le_bytes(b[8..12].try_into().unwrap()) as u64;
        let mut r = Reader(&b[12..]);
        for i in 0..count {
            match r.byte()? {
                1 => {
                    let (k, v) = (r.slice()?, r.slice()?);
                    put(k, seq + i, Some(v));
                }
                0 => put(r.slice()?, seq + i, None),
                _ => return None,
            }
        }
        Some(())
    }

    /// Calls `each(internal_key, value)` for every entry of an `.ldb` table.
    pub fn read_table(
        data: &[u8],
        each: &mut Visit,
    ) -> Result<(), String> {
        if data.len() < 48 {
            return Err("table too short".into());
        }
        let footer = &data[data.len() - 48..];
        if u64::from_le_bytes(footer[40..].try_into().unwrap()) != TABLE_MAGIC {
            return Err("not a table".into());
        }
        let mut r = Reader(footer);
        let _metaindex = (r.varint(), r.varint());
        let index = (r.varint(), r.varint());
        let (Some(off), Some(size)) = index else { return Err("bad footer".into()) };
        let index = read_block(data, off, size)?;
        block_entries(&index, &mut |_, handle| {
            let mut h = Reader(handle);
            let (Some(off), Some(size)) = (h.varint(), h.varint()) else {
                return Err("bad block handle".into());
            };
            let block = read_block(data, off, size)?;
            block_entries(&block, each)
        })
    }

    /// A block's contents, Snappy-decompressed if it was stored that way.
    fn read_block(data: &[u8], off: u64, size: u64) -> Result<Vec<u8>, String> {
        let (off, size) = (off as usize, size as usize);
        let raw = data
            .get(off..off.saturating_add(size).saturating_add(5))
            .ok_or("block past end of table")?;
        let (body, kind) = (&raw[..size], raw[size]);
        if unmask(u32::from_le_bytes(raw[size + 1..].try_into().unwrap())) != crc32c(&raw[..=size]) {
            return Err("block checksum mismatch".into());
        }
        match kind {
            0 => Ok(body.to_vec()),
            1 => super::snappy::decompress(body),
            k => Err(format!("unsupported block compression {k}")),
        }
    }

    /// Entries of a block: prefix-compressed keys, restart array at the end.
    fn block_entries(
        block: &[u8],
        each: &mut Visit,
    ) -> Result<(), String> {
        let bad = || "bad block".to_string();
        let n = block.len().checked_sub(4).ok_or_else(bad)?;
        let restarts = u32::from_le_bytes(block[n..].try_into().unwrap()) as usize;
        let end = n.checked_sub(restarts.checked_mul(4).ok_or_else(bad)?).ok_or_else(bad)?;
        let mut r = Reader(&block[..end]);
        let mut key: Vec<u8> = vec![];
        while !r.0.is_empty() {
            let (shared, unshared, vlen) = (r.varint(), r.varint(), r.varint());
            let (Some(shared), Some(unshared), Some(vlen)) = (shared, unshared, vlen) else {
                return Err(bad());
            };
            if shared as usize > key.len() {
                return Err(bad());
            }
            key.truncate(shared as usize);
            key.extend_from_slice(r.take(unshared as usize).ok_or_else(bad)?);
            let value = r.take(vlen as usize).ok_or_else(bad)?;
            each(&key, value)?;
        }
        Ok(())
    }

    struct Reader<'a>(&'a [u8]);

    impl<'a> Reader<'a> {
        fn byte(&mut self) -> Option<u8> {
            let (&b, rest) = self.0.split_first()?;
            self.0 = rest;
            Some(b)
        }
        fn take(&mut self, n: usize) -> Option<&'a [u8]> {
            if n > self.0.len() {
                return None;
            }
            let (a, rest) = self.0.split_at(n);
            self.0 = rest;
            Some(a)
        }
        fn varint(&mut self) -> Option<u64> {
            let mut v = 0u64;
            for shift in (0..64).step_by(7) {
                let b = self.byte()?;
                v |= ((b & 0x7f) as u64) << shift;
                if b < 0x80 {
                    return Some(v);
                }
            }
            None
        }
        fn slice(&mut self) -> Option<&'a [u8]> {
            let n = self.varint()?;
            self.take(usize::try_from(n).ok()?)
        }
    }

    fn unmask(crc: u32) -> u32 {
        let r = crc.wrapping_sub(0xa282_ead8);
        r.rotate_left(15)
    }

    // shortcut: bitwise CRC-32C, ~10x slower than a table; fine for a one-off read of a few MB.
    pub fn crc32c(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0x82f6_3b78 & (crc & 1).wrapping_neg());
            }
        }
        !crc
    }
}

/// Snappy's raw format (what LevelDB stores), decompression only.
mod snappy {
    pub fn decompress(src: &[u8]) -> Result<Vec<u8>, String> {
        let bad = |what: &str| format!("snappy: {what}");
        let mut pos = 0;
        let mut len = 0usize;
        for shift in (0..35).step_by(7) {
            let b = *src.get(pos).ok_or_else(|| bad("truncated length"))?;
            pos += 1;
            len |= ((b & 0x7f) as usize) << shift;
            if b < 0x80 {
                break;
            }
        }
        let mut out = Vec::with_capacity(len.min(64 << 20));
        let bytes = |pos: &mut usize, n: usize| -> Result<usize, String> {
            let s = src.get(*pos..*pos + n).ok_or_else(|| bad("truncated"))?;
            *pos += n;
            Ok(s.iter().rev().fold(0usize, |acc, &b| acc << 8 | b as usize))
        };
        while pos < src.len() {
            let tag = src[pos];
            pos += 1;
            let (copy_len, offset) = match tag & 3 {
                0 => {
                    let mut n = (tag >> 2) as usize;
                    if n >= 60 {
                        n = bytes(&mut pos, n - 59)?;
                    }
                    let lit = src.get(pos..pos + n + 1).ok_or_else(|| bad("truncated literal"))?;
                    out.extend_from_slice(lit);
                    pos += n + 1;
                    continue;
                }
                1 => (4 + ((tag >> 2) & 7) as usize, ((tag >> 5) as usize) << 8 | bytes(&mut pos, 1)?),
                2 => (1 + (tag >> 2) as usize, bytes(&mut pos, 2)?),
                _ => (1 + (tag >> 2) as usize, bytes(&mut pos, 4)?),
            };
            if offset == 0 || offset > out.len() {
                return Err(bad("bad copy offset"));
            }
            let start = out.len() - offset;
            // Byte by byte: a copy may overlap what it is writing (runs).
            for i in 0..copy_len {
                out.push(out[start + i]);
            }
        }
        if out.len() != len {
            return Err(bad("length mismatch"));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{list_dir, temp_dir};
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chromium").join(name)
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap().flatten() {
            let dest = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &dest);
            } else {
                fs::copy(e.path(), dest).unwrap();
            }
        }
    }

    const WALLPAPER: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    fn small_expected() -> Vec<(String, String)> {
        [
            ("launchpad-accent", "#0078d4"),
            ("launchpad-media-colors", r#"{"title":"Café Ünïcode","emoji":"日本語 🎵"}"#),
            ("launchpad-panel-style", "glass é"),
            ("launchpad-theme", "dark"),
            ("launchpad-topbar-autohide", "true"),
            ("launchpad-tray-order", r#"["volume","wifi","clock"]"#),
            ("launchpad-wallpaper", WALLPAPER),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .to_vec()
    }

    #[test]
    fn reads_the_log_only_fixture() {
        let got = read_local_storage(&fixture("small/leveldb")).unwrap();
        assert_eq!(got, small_expected());
    }

    #[test]
    fn reads_tables_and_log_newest_wins() {
        let dir = fixture("big/leveldb");
        assert!(dir.join("000003.ldb").exists(), "fixture should have a table");
        let got: HashMap<String, String> = read_local_storage(&dir).unwrap().into_iter().collect();
        let mut keys: Vec<&str> = got.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "launchpad-accent",
                "launchpad-big-4",
                "launchpad-big-5",
                "launchpad-big-unicode",
                "launchpad-theme",
                "launchpad-tray-order"
            ]
        );
        assert_eq!(got["launchpad-theme"], "sepia"); // table said "light", log "sepia"
        assert_eq!(got["launchpad-accent"], "#123456");
        assert_eq!(got["launchpad-tray-order"], r#"["a","ü"]"#);
        assert_eq!(got["launchpad-big-4"], "value 4 ".repeat(20000));
        assert_eq!(got["launchpad-big-5"], "value 5 ".repeat(20000));
        assert_eq!(got["launchpad-big-unicode"], "ü日".repeat(30000));
    }

    #[test]
    fn table_blocks_are_snappy_compressed() {
        let data = fs::read(fixture("big/leveldb/000003.ldb")).unwrap();
        let mut n = 0;
        leveldb::read_table(&data, &mut |_, _| {
            n += 1;
            Ok(())
        })
        .unwrap();
        assert!(n > 5);
        // 1.4 MB of values in a 50 KB file: only possible compressed.
        assert!(data.len() < 100_000);
    }

    #[test]
    fn a_torn_log_tail_still_reads() {
        let log = fs::read(fixture("small/leveldb/000003.log")).unwrap();
        let records = leveldb::log_records(&log);
        assert_eq!(records.len(), 2);
        for cut in [log.len() - 1, log.len() - 100, records[0].len() + 7 + 3] {
            assert_eq!(leveldb::log_records(&log[..cut]).len(), 1, "cut at {cut}");
        }
        // Garbage in place of the last record is dropped too.
        let mut bad = log.clone();
        let n = bad.len();
        bad[n - 10] ^= 0xff;
        assert_eq!(leveldb::log_records(&bad).len(), 1);

        let dir = temp_dir();
        copy_dir(&fixture("small/leveldb"), dir.path());
        fs::write(dir.path().join("000003.log"), &log[..log.len() - 50]).unwrap();
        assert_eq!(read_local_storage(dir.path()).unwrap(), vec![]);
    }

    #[test]
    fn snappy_literals_copies_and_runs() {
        // "abcd" literal, then copy-1 offset 4 len 8 (overlapping run).
        let mut src = vec![12u8, 3 << 2, b'a', b'b', b'c', b'd', 1 | (4 << 2), 4];
        assert_eq!(snappy::decompress(&src).unwrap(), b"abcdabcdabcd");
        // copy-2: len 3, offset 2.
        src = vec![5, 1 << 2, b'x', b'y', 2 | (2 << 2), 2, 0];
        assert_eq!(snappy::decompress(&src).unwrap(), b"xyxyx");
        // copy-4: len 2, offset 1.
        src = vec![3, 0, b'z', 3 | (1 << 2), 1, 0, 0, 0];
        assert_eq!(snappy::decompress(&src).unwrap(), b"zzz");
        // Long literal (tag 60: 1-byte length).
        let text = vec![b'q'; 100];
        let mut long = vec![100, 60 << 2, 99];
        long.extend_from_slice(&text);
        assert_eq!(snappy::decompress(&long).unwrap(), text);
        // Errors, not panics.
        assert!(snappy::decompress(&[4, 1, 1]).is_err()); // copy before data
        assert!(snappy::decompress(&[9, 3 << 2, b'a']).is_err()); // truncated literal
        assert!(snappy::decompress(&[5, 0, b'a']).is_err()); // length mismatch
    }

    #[test]
    fn crc32c_known_value() {
        assert_eq!(leveldb::crc32c(b"123456789"), 0xe306_9283);
    }

    #[test]
    fn prefs_round_trip_in_order_and_bad_files_load_empty() {
        let dir = temp_dir();
        assert!(UiPrefs::load(dir.path()).is_empty());
        let mut p = UiPrefs::default();
        p.set("launchpad-theme", "dark");
        p.set("launchpad-accent", "#fff");
        p.set("launchpad-x", "1");
        p.remove("launchpad-x");
        p.save(dir.path()).unwrap();
        let back = UiPrefs::load(dir.path());
        assert_eq!(back, p);
        assert_eq!(back.keys().collect::<Vec<_>>(), ["launchpad-theme", "launchpad-accent"]);
        assert_eq!(back.get("launchpad-accent"), Some("#fff"));
        fs::write(dir.path().join(UI_FILE), "{not json").unwrap();
        assert!(UiPrefs::load(dir.path()).is_empty());
    }

    #[test]
    fn migrate_writes_ui_json_and_wallpaper_and_deletes_only_chromium() {
        let dir = temp_dir();
        let d = dir.path();
        copy_dir(&fixture("small"), &d.join("Local Storage"));
        let ours = [
            "apps.json", "apps.json.bak", "settings.json", "layouts.json", "library.json",
            "profiles.json", "appbars.json", "unknown.txt", "Local Storage.txt",
        ];
        for f in ours {
            fs::write(d.join(f), "{}").unwrap();
        }
        for f in ["icons", "files", "thumbs", "art", "pre-reset-backup"] {
            fs::create_dir_all(d.join(f).join("sub")).unwrap();
        }
        for f in ["Cache", "GPUCache", "Code Cache", "Network", "blob_storage"] {
            fs::create_dir_all(d.join(f).join("x")).unwrap();
            fs::write(d.join(f).join("x/data_0"), "x").unwrap();
        }
        for f in ["Local State", "Preferences", "DIPS", ".updaterId"] {
            fs::write(d.join(f), "x").unwrap();
        }

        let m = migrate(d).unwrap();
        assert!(!m.already_done);
        assert_eq!(m.read_error, None);
        assert_eq!(m.wallpaper.as_deref(), Some("wallpaper.png"));
        assert_eq!(m.keys.len(), 7);
        let mut deleted = m.deleted.clone();
        deleted.sort();
        assert_eq!(
            deleted,
            [".updaterId", "Cache", "Code Cache", "DIPS", "GPUCache", "Local State",
             "Local Storage", "Network", "Preferences", "blob_storage"]
        );

        let prefs = UiPrefs::load(d);
        for (k, v) in small_expected() {
            let want = if k == WALLPAPER_KEY { "wallpaper.png" } else { v.as_str() };
            assert_eq!(prefs.get(&k), Some(want), "{k}");
        }
        assert_eq!(fs::read(d.join("wallpaper.png")).unwrap(), base64_decode(&WALLPAPER[22..]).unwrap());
        assert_eq!(&fs::read(d.join("wallpaper.png")).unwrap()[1..4], b"PNG");

        let mut left = list_dir(d);
        left.sort();
        let mut want: Vec<String> = ours.iter().map(|s| s.to_string()).collect();
        want.extend(["icons", "files", "thumbs", "art", "pre-reset-backup", "ui.json", "wallpaper.png"].map(String::from));
        want.sort();
        assert_eq!(left, want);
        for f in ["icons", "files", "thumbs", "art", "pre-reset-backup"] {
            assert!(d.join(f).join("sub").is_dir());
        }
    }

    #[test]
    fn migrate_is_a_no_op_once_ui_json_exists() {
        let dir = temp_dir();
        let d = dir.path();
        copy_dir(&fixture("small"), &d.join("Local Storage"));
        fs::create_dir(d.join("Cache")).unwrap();
        fs::write(d.join(UI_FILE), r#"{"launchpad-theme":"light"}"#).unwrap();
        let m = migrate(d).unwrap();
        assert!(m.already_done);
        assert!(d.join("Cache").is_dir() && d.join("Local Storage").is_dir());
        assert_eq!(UiPrefs::load(d).get("launchpad-theme"), Some("light"));
    }

    #[test]
    fn migrate_without_local_storage_still_starts() {
        let dir = temp_dir();
        let m = migrate(dir.path()).unwrap();
        assert!(m.read_error.is_some() && m.keys.is_empty());
        assert!(UiPrefs::load(dir.path()).is_empty() && dir.path().join(UI_FILE).exists());
    }

    #[test]
    fn unreadable_local_storage_is_kept() {
        let dir = temp_dir();
        let ls = dir.path().join("Local Storage").join("leveldb");
        fs::create_dir_all(&ls).unwrap();
        fs::write(ls.join("CURRENT"), "MANIFEST-000099
").unwrap();
        fs::write(ls.join("000001.ldb"), b"not a table").unwrap();
        fs::create_dir(dir.path().join("GPUCache")).unwrap();
        let m = migrate(dir.path()).unwrap();
        assert!(m.read_error.is_some(), "{m:?}");
        assert!(dir.path().join("Local Storage").is_dir() && !dir.path().join("GPUCache").exists());
    }

    #[test]
    fn data_urls() {
        assert_eq!(decode_image_data_url("data:image/jpeg;base64,/9j/").unwrap(), ("jpg", vec![0xff, 0xd8, 0xff]));
        assert!(decode_image_data_url("data:image/svg+xml;base64,AAAA").is_none());
        assert!(decode_image_data_url("data:image/png,raw").is_none());
        assert!(decode_image_data_url("data:image/png;base64,@@").is_none());
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
    }
}
