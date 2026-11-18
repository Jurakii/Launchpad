//! What launchpad-core asks the app for, wired to the Windows layer: thin
//! adapters over `image`, `http`, `crypto` and `launch`. All of them block
//! (network, shell, disk): use them on a worker thread, never the UI thread.
//! The image ones hold a `Wic`, so each lives on the thread that made it.
use super::http;
use super::image::{large_icon_size, Format, Rgba, Source, Wic};
use super::launch::{self, Tracked};
use super::{arg, crypto};
use launchpad_core::favicon::{self, IconSource, FAVICON_SIZE};
use launchpad_core::file_store::Thumbnails;
use launchpad_core::media::art::{self as art, ArtHttp, ArtImages, ArtResponse, JPEG_QUALITY};
use launchpad_core::media::launch::{Child, Host};
use launchpad_core::media::library::Entry;
use launchpad_core::media::players::Launch;
use launchpad_core::update::{self, HttpResponse};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// Site icons: core's `fetch_favicon` over WinHTTP and WIC. Images come back
/// as `Rgba`, at most 128 px (larger ones scaled to 128x128 as icons.js did).
pub struct Icons {
    wic: Wic,
    start: Instant,
}

impl Icons {
    pub fn new() -> Result<Icons> {
        Ok(Icons { wic: Wic::new()?, start: Instant::now() })
    }

    /// The PNG to save as `icons\<id>.png`.
    pub fn png(&self, img: &Rgba) -> Result<Vec<u8>> {
        self.wic.encode_rgba(img, Format::Png)
    }
}

impl IconSource for Icons {
    type Image = Rgba;

    fn get(&mut self, url: &str, max_bytes: usize, timeout_ms: u64) -> Option<favicon::Response> {
        let r = http::get(url, max_bytes, timeout_ms).ok()?;
        Some(favicon::Response { ok: (200..=299).contains(&r.status), url: r.url, content_type: r.content_type, body: r.body })
    }

    fn decode(&mut self, bytes: &[u8]) -> Option<Rgba> {
        let fit = |w: u32, h: u32| if w > FAVICON_SIZE { (FAVICON_SIZE, FAVICON_SIZE) } else { (w, h) };
        self.wic.decode_rgba(bytes, fit).ok()
    }

    fn now_ms(&mut self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// Media art: decoding stays lazy until `encode`, so only the slot's size is
/// ever held in memory.
pub struct Art(pub Wic);

impl ArtImages for Art {
    type Image = Source;

    fn decode(&mut self, bytes: &[u8]) -> Option<Source> {
        self.0.decode(bytes).ok()
    }

    fn size(&self, image: &Source) -> (u32, u32) {
        self.0.size(image)
    }

    fn resize(&mut self, image: Source, width: u32, height: u32) -> Source {
        // shortcut: on a WIC failure the unscaled image is kept (the trait can't fail); encode then reports it.
        self.0.resize(&image, width, height).unwrap_or(image)
    }

    fn encode(&mut self, image: &Source, ext: &str) -> Vec<u8> {
        let format = if ext == "png" { Format::Png } else { Format::Jpeg(JPEG_QUALITY) };
        // An empty file fails core's decode next time rather than panicking now.
        self.0.encode(image, format).unwrap_or_default()
    }
}

/// HTTP for Media art downloads.
pub struct Http;

impl ArtHttp for Http {
    fn get(&mut self, url: &str, max_bytes: usize, timeout_ms: u64) -> Option<ArtResponse> {
        let r = http::get(url, max_bytes, timeout_ms).ok()?;
        Some(ArtResponse { status: r.status, content_length: r.content_length, body: r.body })
    }
}

/// Stored-file thumbnails and icons, as PNG.
pub struct Thumbs(pub Wic);

impl Thumbnails for Thumbs {
    fn thumbnail(&mut self, path: &Path, size: u32) -> Option<Vec<u8>> {
        self.0.encode_rgba(&self.0.thumbnail(path, size).ok()?, Format::Png).ok()
    }

    fn file_icon(&mut self, path: &Path) -> Option<Vec<u8>> {
        self.0.encode_rgba(&self.0.file_icon(path, large_icon_size()).ok()?, Format::Png).ok()
    }
}

/// Update manifests and installers: a few MB, so a generous cap and time.
pub const UPDATE_MAX_BYTES: usize = 256 * 1024 * 1024;
pub const UPDATE_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// The `fetch` for `update::check_for_update` and `update::download`.
pub fn update_fetch(url: &str) -> Result<HttpResponse> {
    let r = http::get(url, UPDATE_MAX_BYTES, UPDATE_TIMEOUT_MS)?;
    Ok(HttpResponse { status: r.status, body: r.body })
}

/// The `sha512` for `update::download`. CNG doesn't fail on a working
/// Windows; if it did, the zero hash fails the checksum check with core's message.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    crypto::sha512(data).unwrap_or([0; 64])
}

impl Child for Tracked {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn kill(&mut self) {
        Tracked::kill(self);
    }
}

/// Media's process host. `on_exit(pid, code)` runs on a waiter thread when a
/// tracked process ends; the app maps the pid to the entry (`Launcher::list`)
/// and calls `Launcher::exited` on its own thread.
pub struct MediaHost {
    pub on_exit: Arc<dyn Fn(u32, Option<i32>) + Send + Sync>,
}

impl Host for MediaHost {
    type Child = Tracked;

    fn spawn(&mut self, l: &Launch) -> Result<Tracked> {
        let on_exit = self.on_exit.clone();
        launch::spawn_tracked(&l.exe, &l.args, &l.cwd, move |pid, code| on_exit(pid, code))
    }

    fn launch_other(&mut self, entry: &Entry) -> Result<()> {
        launch::launch_item(&entry.0)
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn now(&self) -> i64 {
        launchpad_core::time::now_ms()
    }
}

/// `favicon --url U [--google] --out f.png` runs core's fetch_favicon for
/// real; `update-check` runs core's check_for_update against GitHub;
/// `update-download` core's download (writes only into --dir); the rest run
/// the Thumbnails, ArtImages/ArtHttp and Media Host adapters once.
pub fn probe(args: &[String]) -> Result<Value> {
    match args.first().map(String::as_str) {
        Some("favicon") => {
            let url = arg(args, "--url").ok_or_else(|| Error::msg("--url is required"))?;
            let out = arg(args, "--out").ok_or_else(|| Error::msg("--out is required"))?;
            let mut icons = Icons::new()?;
            let start = Instant::now();
            let img = favicon::fetch_favicon(url, args.iter().any(|a| a == "--google"), &mut icons)
                .ok_or_else(|| Error::msg(format!("No icon found for {url}")))?;
            let png = icons.png(&img)?;
            std::fs::write(out, &png)?;
            Ok(json!({ "width": img.width, "height": img.height, "bytes": png.len(), "ms": start.elapsed().as_millis() as u64, "out": out }))
        }
        Some("update-check") => {
            let current = arg(args, "--current").unwrap_or(env!("CARGO_PKG_VERSION"));
            let newer = update::check_for_update(current, update_fetch)?;
            Ok(json!({ "current": current, "manifest": update::latest_yml_url(), "newer": newer }))
        }
        Some("update-download") => {
            let version = arg(args, "--version").ok_or_else(|| Error::msg("--version is required"))?;
            let dir = arg(args, "--dir").ok_or_else(|| Error::msg("--dir is required"))?;
            let path = update::download(version, update_fetch, sha512, Path::new(dir))?;
            Ok(json!({ "installer": path }))
        }
        Some("thumbs") => {
            // As core's make_thumbnail: the thumbnail, else the file's icon.
            let path = Path::new(arg(args, "--path").ok_or_else(|| Error::msg("--path is required"))?);
            let out = arg(args, "--out").ok_or_else(|| Error::msg("--out is required"))?;
            let mut t = Thumbs(Wic::new()?);
            let (kind, png) = match t.thumbnail(path, launchpad_core::file_store::THUMB_SIZE) {
                Some(png) => ("thumbnail", png),
                None => ("icon", t.file_icon(path).ok_or_else(|| Error::msg("No thumbnail or icon"))?),
            };
            std::fs::write(out, &png)?;
            Ok(json!({ "kind": kind, "bytes": png.len(), "out": out }))
        }
        Some("art") => {
            // What core's save_art does, without the library folder: fit to the slot, encode.
            let slot = arg(args, "--slot").unwrap_or("cover");
            let spec = art::slot_spec(slot).ok_or_else(|| Error::msg("Unknown slot"))?;
            let bytes = match (arg(args, "--in"), arg(args, "--url")) {
                (Some(file), _) => art::read_image_file(Path::new(file))?,
                (None, Some(url)) => art::download_image(url, &mut Http)?,
                _ => return Err(Error::msg("--in or --url is required")),
            };
            let out = arg(args, "--out").ok_or_else(|| Error::msg("--out is required"))?;
            let mut a = Art(Wic::new()?);
            let img = a.decode(&bytes).ok_or_else(|| Error::msg("That image couldn't be read"))?;
            let (w, h) = a.size(&img);
            let (fw, fh) = art::fit_size(w, h, spec.w, spec.h);
            let img = a.resize(img, fw, fh);
            let data = a.encode(&img, spec.ext);
            std::fs::write(out, &data)?;
            Ok(json!({ "from": [w, h], "to": [fw, fh], "ext": spec.ext, "bytes": data.len(), "out": out }))
        }
        Some("media") => {
            // A tracked launch as Media makes it; waits for the exit report.
            let exe = arg(args, "--exe").ok_or_else(|| Error::msg("--exe is required"))?;
            let l = Launch {
                exe: exe.into(),
                args: args.windows(2).filter(|w| w[0] == "--arg").map(|w| w[1].clone()).collect(),
                cwd: arg(args, "--cwd").unwrap_or("").into(),
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let tx = std::sync::Mutex::new(tx);
            let mut host = MediaHost { on_exit: Arc::new(move |pid, code| drop(tx.lock().map(|t| t.send((pid, code))))) };
            let child = host.spawn(&l)?;
            let pid = Child::pid(&child);
            let (exited, code) = rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|_| Error::msg("No exit within 30 s"))?;
            Ok(json!({ "pid": pid, "exitedPid": exited, "code": code, "exists": host.exists(exe), "now": host.now() }))
        }
        _ => Err(Error::msg(
            "Usage: --probe services favicon --url U [--google] --out f.png | update-check [--current V] | update-download --version V --dir D | thumbs --path P --out f.png | art (--in F | --url U) [--slot S] --out F | media --exe P [--arg A]… [--cwd D]",
        )),
    }
}
