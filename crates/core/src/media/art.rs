//! Artwork for library entries: one image per slot in media\<id>\, resized
//! on the way in so a 4K banner doesn't cost 4K on every tile. Each slot
//! remembers who set it ('user' or 'scraper:<source>') so a scrape never
//! replaces art the user picked. Ported from lib/media/art.js; decoding,
//! resizing, encoding and HTTP come in through [`ArtImages`] and [`ArtHttp`].
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::library::{is_valid_id, Entry, Library};
use super::truthy;
use crate::favicon::{is_jpeg, is_png, Url};
use crate::safe_file::write_file_atomic;
use crate::{Error, Result};

/// Largest size kept per slot (fit inside, never enlarged) and the format:
/// logos keep transparency, the rest are photos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub w: u32,
    pub h: u32,
    pub ext: &'static str,
}

pub const IMAGE_SLOTS: [&str; 4] = ["cover", "banner", "logo", "screenshot"];
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_DOWNLOAD_BYTES: usize = 15 * 1024 * 1024;
pub const DOWNLOAD_TIMEOUT_MS: u64 = 20_000;
pub const JPEG_QUALITY: u8 = 88;

/// `SLOTS[slot]`.
pub fn slot_spec(slot: &str) -> Option<Slot> {
    let (w, h, ext) = match slot {
        "cover" => (600, 900, "jpg"),
        "banner" => (1920, 1080, "jpg"),
        "logo" => (800, 400, "png"),
        "screenshot" => (1280, 720, "jpg"),
        _ => return None,
    };
    Some(Slot { w, h, ext })
}

/// Image decoding and encoding, supplied by the app (WIC in Phase 2).
pub trait ArtImages {
    type Image;

    /// Decodes PNG or JPEG bytes. `None` if they don't decode.
    fn decode(&mut self, bytes: &[u8]) -> Option<Self::Image>;

    /// Width and height in pixels.
    fn size(&self, image: &Self::Image) -> (u32, u32);

    /// Scaled to exactly `width` x `height`, best quality.
    fn resize(&mut self, image: Self::Image, width: u32, height: u32) -> Self::Image;

    /// PNG bytes for `ext == "png"`, else JPEG at [`JPEG_QUALITY`].
    fn encode(&mut self, image: &Self::Image, ext: &str) -> Vec<u8>;
}

pub struct ArtResponse {
    pub status: u16,
    /// The Content-Length header, if there was one.
    pub content_length: Option<u64>,
    pub body: Vec<u8>,
}

/// The network, supplied by the app.
pub trait ArtHttp {
    /// GET `url`, following redirects, reading at most `max_bytes` of the
    /// body and then dropping the connection. `None` if the request failed
    /// or took longer than `timeout_ms`.
    fn get(&mut self, url: &str, max_bytes: usize, timeout_ms: u64) -> Option<ArtResponse>;
}

fn check_slot(slot: &str) -> Result<Slot> {
    slot_spec(slot).ok_or_else(|| Error::msg(format!("Unknown art slot: {slot}")))
}

pub fn file_name_for(slot: &str) -> Result<String> {
    Ok(format!("{slot}.{}", check_slot(slot)?.ext))
}

/// The size that fits (w, h) inside (max_w, max_h) keeping its shape; never larger.
pub fn fit_size(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let (w, h) = (f64::from(w), f64::from(h));
    let scale = 1f64.min(f64::from(max_w) / w).min(f64::from(max_h) / h);
    let side = |n: f64| crate::js::js_round(n * scale).max(1.0) as u32;
    (side(w), side(h))
}

pub fn art_path(dir: &Path, id: &str, slot: &str) -> Result<PathBuf> {
    let folder = Library::new(dir).media_dir_for(id)?; // id checked before the slot, as in the JS
    Ok(folder.join(file_name_for(slot)?))
}

/// Resizes and stores an image for one slot. Returns the slot's new value
/// for entry.media: `{ path, source, updatedAt }`, path relative to the
/// entry's media folder.
pub fn save_art<I: ArtImages>(
    dir: &Path,
    id: &str,
    slot: &str,
    buf: &[u8],
    source: &str,
    now: i64,
    images: &mut I,
) -> Result<Value> {
    let spec = check_slot(slot)?;
    // PNG or JPEG only: the decoder isn't asked about anything else.
    if !is_png(buf) && !is_jpeg(buf) {
        return Err(Error::msg("Use a PNG or JPEG image"));
    }
    let img = images.decode(buf).ok_or_else(|| Error::msg("That image couldn't be read"))?;
    let (width, height) = images.size(&img);
    let (fit_w, fit_h) = fit_size(width, height, spec.w, spec.h);
    let out = if (fit_w, fit_h) == (width, height) { img } else { images.resize(img, fit_w, fit_h) };
    let file = art_path(dir, id, slot)?;
    // Written next to the target and renamed over it, so a tile never loads
    // a half-written image.
    fs::create_dir_all(file.parent().expect("art paths have a folder"))?;
    write_file_atomic(&file, images.encode(&out, spec.ext))?;
    Ok(json!({ "path": file_name_for(slot)?, "source": source, "updatedAt": now }))
}

pub fn read_image_file(file: &Path) -> Result<Vec<u8>> {
    // Node's win32 isAbsolute: a root, with or without a drive.
    if !file.has_root() {
        return Err(Error::msg("Choose an image file"));
    }
    let stat = fs::metadata(file).map_err(|_| Error::msg("Can't find that file"))?;
    if !stat.is_file() {
        return Err(Error::msg("Choose an image file"));
    }
    if stat.len() > MAX_FILE_BYTES {
        return Err(Error::msg("That image is over 25 MB"));
    }
    Ok(fs::read(file)?)
}

// `new URL(url)` failing vs succeeding with another scheme. Url::parse only
// says "not http(s)", so a valid scheme prefix is taken to mean "a URL, just
// not a web one".
fn has_other_scheme(url: &str) -> bool {
    let text = url.trim_matches(|c: char| c <= ' ');
    let Some(colon) = text.find(':') else { return false };
    let scheme = &text[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !scheme.eq_ignore_ascii_case("http")
        && !scheme.eq_ignore_ascii_case("https")
}

/// Downloads an image over http(s), at most [`MAX_DOWNLOAD_BYTES`] of it.
pub fn download_image<H: ArtHttp>(url: &str, http: &mut H) -> Result<Vec<u8>> {
    let parsed = match Url::parse(url) {
        Some(u) => u,
        None if has_other_scheme(url) => return Err(Error::msg("Use an http or https address")),
        None => return Err(Error::msg("That is not a web address")),
    };
    let res = http
        .get(&parsed.href(), MAX_DOWNLOAD_BYTES + 1, DOWNLOAD_TIMEOUT_MS)
        .ok_or_else(|| Error::msg("Couldn't download that image"))?;
    if !(200..=299).contains(&res.status) {
        return Err(Error::msg(format!("The site answered {}", res.status)));
    }
    if res.content_length.is_some_and(|n| n > MAX_DOWNLOAD_BYTES as u64) || res.body.len() > MAX_DOWNLOAD_BYTES {
        return Err(Error::msg("That image is over 15 MB"));
    }
    Ok(res.body)
}

pub fn clear_art(dir: &Path, id: &str, slot: &str) -> Result<()> {
    match fs::remove_file(art_path(dir, id, slot)?) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// The art a library entry may show: slots whose file is really there.
pub fn existing_art(dir: &Path, entry: &Entry) -> Map<String, Value> {
    let mut out = Map::new();
    let media = entry.0.get("media").and_then(Value::as_object);
    for slot in IMAGE_SLOTS {
        let m = media.and_then(|m| m.get(slot));
        if truthy(m) && is_valid_id(entry.id()) && art_path(dir, entry.id(), slot).is_ok_and(|p| p.exists()) {
            out.insert(slot.into(), m.cloned().unwrap_or_default());
        }
    }
    out
}

/// Stores new art for one slot of one entry, or clears it (`buf` None), and
/// returns the updated entry. (main.js's setEntryArt.)
pub fn set_entry_art<I: ArtImages>(
    library: &mut Library,
    id: &str,
    slot: &str,
    buf: Option<&[u8]>,
    source: &str,
    images: &mut I,
) -> Result<Entry> {
    if !is_valid_id(id) {
        return Err(Error::msg("Invalid library id"));
    }
    if slot_spec(slot).is_none() {
        return Err(Error::msg("Unknown art slot"));
    }
    let mut entries = library.load()?;
    let dir = library.dir().to_owned();
    let Some(entry) = entries.iter_mut().find(|e| e.0.get("id").and_then(Value::as_str) == Some(id)) else {
        return Err(Error::msg("That item is no longer in the library"));
    };
    let value = match buf {
        Some(buf) => save_art(&dir, id, slot, buf, source, crate::time::now_ms(), images)?,
        None => {
            clear_art(&dir, id, slot)?;
            Value::Null
        }
    };
    // normalize() always leaves media an object.
    if let Some(Value::Object(media)) = entry.0.get_mut("media") {
        media.insert(slot.into(), value);
    } else {
        entry.0.insert("media".into(), json!({ slot: value }));
    }
    let updated = entry.clone();
    library.save(&entries)?;
    Ok(updated)
}

// WHATWG path parsing for a non-special URL: "." and ".." (also as %2e)
// are resolved, nothing is decoded, and `\` is an ordinary character.
fn path_segments(path: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut parts = path.strip_prefix('/').unwrap_or(path).split('/').peekable();
    while let Some(seg) = parts.next() {
        let last = parts.peek().is_none();
        let lower = seg.to_ascii_lowercase();
        if matches!(lower.as_str(), ".." | ".%2e" | "%2e." | "%2e%2e") {
            out.pop();
            if last {
                out.push("");
            }
        } else if matches!(lower.as_str(), "." | "%2e") {
            if last {
                out.push("");
            }
        } else {
            out.push(seg);
        }
    }
    out
}

/// `lpmedia://art/<uuid>/<slot>[?v=...]` -> the file to serve, or None for
/// any other shape. Only those two parts are read, both checked against
/// fixed patterns, so no other file can be reached through it. The
/// protocol is gone in the Rust build; this stays as the check for whatever
/// serves art to the UI.
pub fn resolve_art_url(dir: &Path, url: &str) -> Option<PathBuf> {
    let text: String = url.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let (scheme, rest) = text.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("lpmedia") {
        return None;
    }
    let rest = rest.strip_prefix("//")?;
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = host_port.split_once(':').unwrap_or((host_port, ""));
    // The host of a non-special URL is kept as written, so "ART" isn't "art".
    if host != "art" || !port.chars().all(|c| c.is_ascii_digit()) || port.parse::<u32>().is_ok_and(|p| p > 65535) {
        return None;
    }
    let parts: Vec<&str> = path_segments(path).into_iter().filter(|s| !s.is_empty()).collect();
    let [id, slot] = parts[..] else { return None };
    if !is_valid_id(id) || !IMAGE_SLOTS.contains(&slot) {
        return None;
    }
    art_path(dir, id, slot).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::test_entry;
    use crate::test_util::temp_dir;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];

    /// Decoder stand-in: reports a size and records the resizes asked for.
    struct FakeImages {
        size: (u32, u32),
        calls: Vec<(u32, u32)>,
    }

    fn fake(size: (u32, u32)) -> FakeImages {
        FakeImages { size, calls: vec![] }
    }

    impl ArtImages for FakeImages {
        type Image = (u32, u32);
        fn decode(&mut self, _: &[u8]) -> Option<(u32, u32)> {
            Some(self.size)
        }
        fn size(&self, image: &(u32, u32)) -> (u32, u32) {
            *image
        }
        fn resize(&mut self, _: (u32, u32), width: u32, height: u32) -> (u32, u32) {
            self.calls.push((width, height));
            (width, height)
        }
        fn encode(&mut self, &(w, h): &(u32, u32), ext: &str) -> Vec<u8> {
            format!("{} {w}x{h}", if ext == "png" { "png" } else { "jpeg" }).into_bytes()
        }
    }

    fn msg(r: Result<impl std::fmt::Debug>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn fit_size_shrinks_to_fit_and_never_enlarges() {
        assert_eq!(fit_size(3000, 1500, 1920, 1080), (1920, 960));
        assert_eq!(fit_size(1000, 2000, 600, 900), (450, 900));
        assert_eq!(fit_size(300, 200, 600, 900), (300, 200));
    }

    #[test]
    fn save_art_resizes_into_the_slot_box_and_writes_slot_named_files() {
        let dir = temp_dir();
        let mut images = fake((3000, 1500));
        let value = save_art(dir.path(), ID, "banner", PNG, "user", 99, &mut images).unwrap();
        assert_eq!(value, json!({ "path": "banner.jpg", "source": "user", "updatedAt": 99 }));
        assert_eq!(images.calls[0], (1920, 960));
        let file = dir.path().join("media").join(ID).join("banner.jpg");
        assert_eq!(fs::read_to_string(&file).unwrap(), "jpeg 1920x960");
        assert!(!dir.path().join("media").join(ID).join("banner.jpg.tmp").exists());

        save_art(dir.path(), ID, "logo", JPEG, "user", 1, &mut images).unwrap();
        let logo = fs::read_to_string(dir.path().join("media").join(ID).join("logo.png")).unwrap();
        assert_eq!(logo, "png 800x400", "logos stay PNG");
    }

    #[test]
    fn save_art_refuses_unknown_slots_bad_ids_and_non_png_jpeg_data() {
        let dir = temp_dir();
        let mut images = fake((3000, 1500));
        assert!(msg(save_art(dir.path(), ID, "poster", PNG, "user", 1, &mut images)).contains("Unknown art slot"));
        assert!(msg(save_art(dir.path(), "..", "cover", PNG, "user", 1, &mut images)).contains("Invalid library id"));
        assert!(msg(save_art(dir.path(), ID, "cover", b"GIF89a", "user", 1, &mut images)).contains("PNG or JPEG"));
    }

    #[test]
    fn clear_art_removes_the_file_and_existing_art_only_lists_files_that_exist() {
        let dir = temp_dir();
        let cover = save_art(dir.path(), ID, "cover", PNG, "user", 1, &mut fake((3000, 1500))).unwrap();
        let entry = test_entry(
            json!({ "id": ID, "media": { "cover": cover, "banner": { "path": "banner.jpg", "source": "user" }, "logo": null } }),
        );
        assert_eq!(existing_art(dir.path(), &entry).keys().collect::<Vec<_>>(), ["cover"]);
        clear_art(dir.path(), ID, "cover").unwrap();
        assert_eq!(existing_art(dir.path(), &entry), Map::new());
        clear_art(dir.path(), ID, "cover").unwrap(); // already gone: no error
    }

    #[test]
    fn read_image_file_checks_the_path_before_reading() {
        let dir = temp_dir();
        let file = dir.path().join("pic.png");
        fs::write(&file, PNG).unwrap();
        assert_eq!(read_image_file(&file).unwrap(), PNG);
        assert!(msg(read_image_file(Path::new("pic.png"))).contains("Choose an image file"));
        assert!(msg(read_image_file(&dir.path().join("missing.png"))).contains("Can't find"));
        assert!(msg(read_image_file(dir.path())).contains("Choose an image file"));
    }

    #[test]
    fn resolve_art_url_only_maps_lpmedia_art_uuid_slot_into_the_media_folder() {
        let dir = temp_dir();
        let media = dir.path().join("media");
        let resolve = |u: &str| resolve_art_url(dir.path(), u);
        assert_eq!(resolve(&format!("lpmedia://art/{ID}/cover?v=5")), Some(media.join(ID).join("cover.jpg")));
        assert_eq!(resolve(&format!("lpmedia://art/{}/logo", ID.to_uppercase())), Some(media.join(ID).join("logo.png")));
        for bad in [
            format!("lpmedia://art/{ID}/poster"),
            format!("lpmedia://art/{ID}"),
            format!("lpmedia://art/{ID}/cover/extra"),
            format!("lpmedia://other/{ID}/cover"),
            format!("lpmedia://art/../{ID}/cover"),
            "lpmedia://art/%2e%2e/cover".into(),
            "lpmedia://art/not-an-id/cover".into(),
            "file:///C:/Windows/win.ini".into(),
            "not a url".into(),
        ] {
            let resolved = resolve(&bad);
            assert!(resolved.as_ref().is_none_or(|p| p.starts_with(&media)), "{bad}");
            if bad.contains("..") || bad.contains("%2e") {
                assert!(resolved.is_none() || resolved == Some(media.join(ID).join("cover.jpg")), "{bad}");
            }
        }
        assert_eq!(resolve(&format!("lpmedia://art/{ID}/poster")), None);
        assert_eq!(resolve("file:///C:/Windows/win.ini"), None);
    }

    // Not in the JS tests: URL shapes checked against node's `new URL`.
    #[test]
    fn resolve_art_url_follows_whatwg_parsing() {
        let dir = temp_dir();
        let cover = Some(dir.path().join("media").join(ID).join("cover.jpg"));
        let resolve = |u: String| resolve_art_url(dir.path(), &u);
        assert_eq!(resolve(format!("LPMEDIA://art/{ID}/cover#x")), cover);
        assert_eq!(resolve(format!("lpmedia://art/../{ID}/cover")), cover);
        assert_eq!(resolve(format!("lpmedia://art/x/%2E%2e/{ID}/./cover")), cover);
        assert_eq!(resolve(format!("lpmedia://u@art:80/{ID}//cover")), cover);
        assert_eq!(resolve(format!("lpmedia://ART/{ID}/cover")), None);
        assert_eq!(resolve(format!("lpmedia:art/{ID}/cover")), None);
        assert_eq!(resolve(format!("lpmedia://art/{ID}\\..\\x/cover")), None);
        assert_eq!(resolve(format!("lpmedia://art/a/{ID}/cover/..")), None);
    }

    struct FakeHttp(Option<ArtResponse>, Vec<String>);

    impl ArtHttp for FakeHttp {
        fn get(&mut self, url: &str, max_bytes: usize, _: u64) -> Option<ArtResponse> {
            assert_eq!(max_bytes, MAX_DOWNLOAD_BYTES + 1);
            self.1.push(url.to_owned());
            self.0.take()
        }
    }

    // Not in the JS tests (downloadImage has none): the messages for each way
    // a download goes wrong.
    #[test]
    fn download_image_refuses_non_web_addresses_errors_and_big_bodies() {
        let res = |status, content_length, len| Some(ArtResponse { status, content_length, body: vec![0; len] });
        let run = |url: &str, r| download_image(url, &mut FakeHttp(r, vec![]));
        assert_eq!(msg(run("nope", None)), "That is not a web address");
        assert_eq!(msg(run("ftp://x.com/a.png", None)), "Use an http or https address");
        assert_eq!(msg(run("https://x.com/a.png", None)), "Couldn't download that image");
        assert_eq!(msg(run("https://x.com/a.png", res(404, None, 0))), "The site answered 404");
        assert_eq!(msg(run("https://x.com/a.png", res(200, Some(20 << 20), 0))), "That image is over 15 MB");
        assert_eq!(msg(run("https://x.com/a.png", res(200, None, MAX_DOWNLOAD_BYTES + 1))), "That image is over 15 MB");
        let mut http = FakeHttp(res(200, Some(3), 3), vec![]);
        assert_eq!(download_image("HTTPS://X.com/a.png", &mut http).unwrap(), vec![0; 3]);
        assert_eq!(http.1, ["https://x.com/a.png"]);
    }

    // Not in the JS tests: main.js's setEntryArt, now set_entry_art.
    #[test]
    fn set_entry_art_saves_into_the_entry_and_clears_it() {
        let dir = temp_dir();
        let mut library = Library::new(dir.path());
        library.replace_all(vec![json!({ "id": ID, "name": "Game" })]).unwrap();
        let mut images = fake((10, 10));
        let entry = set_entry_art(&mut library, ID, "cover", Some(PNG), "user", &mut images).unwrap();
        assert_eq!(entry.0["media"]["cover"]["path"], "cover.jpg");
        assert_eq!(library.load().unwrap()[0].0["media"]["cover"]["source"], "user");
        assert!(dir.path().join("media").join(ID).join("cover.jpg").exists());

        let entry = set_entry_art(&mut library, ID, "cover", None, "", &mut images).unwrap();
        assert_eq!(entry.0["media"]["cover"], Value::Null);
        assert!(!dir.path().join("media").join(ID).join("cover.jpg").exists());
        let missing = "22222222-2222-4222-8222-222222222222";
        assert_eq!(msg(set_entry_art(&mut library, missing, "cover", None, "", &mut images)), "That item is no longer in the library");
        assert_eq!(msg(set_entry_art(&mut library, ID, "video", None, "", &mut images)), "Unknown art slot");
    }
}
