//! Images through WIC and the shell: decode PNG/JPEG/ICO, scale, encode PNG
//! and JPEG, file icons and thumbnails (RUST_PORT_PLAN.md §4.2, §4.3).
//!
//! WIC decodes lazily: a [`Source`] is a pipeline (decoder frame -> scaler ->
//! converter) and pixels are only pulled through it, at the final size, when
//! it is encoded or turned into an [`Rgba`]. So a 4K banner saved at 600x900
//! is never held at 4K.
//!
//! Everything here blocks (the shell can take a while on a video thumbnail):
//! call it off the UI thread. Objects are apartment-bound: use a `Wic` and its
//! `Source`s on the thread that made them, and drop the `Source`s first.
use super::{arg, wide, Com, Context};
use launchpad_core::favicon::ico_entries;
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::path::Path;
use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, HPALETTE};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::StructuredStorage::{IPropertyBag2, PROPBAG2};
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_R4};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHCreateMemStream, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON,
    SHGFI_LARGEICON, SIIGBF, SIIGBF_ICONONLY, SIIGBF_THUMBNAILONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetSystemMetrics, SM_CXICON};

/// Pixels ready for the UI: 8-bit RGBA, **straight (not premultiplied)**
/// alpha, rows top-down, stride `width * 4`. What Slint's
/// `SharedPixelBuffer<Rgba8Pixel>` / `Image::from_rgba8` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A decoded (lazily) image. See the module docs.
pub struct Source(IWICBitmapSource);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    /// Quality 0-100.
    Jpeg(u8),
}

/// The WIC factory, with COM held for this thread while it lives.
pub struct Wic {
    // Declared before `_com` so it is released before COM is.
    f: IWICImagingFactory,
    _com: Com,
}

/// The system's large icon size (32 px at 100 % scaling): what Electron's
/// `app.getFileIcon(p, { size: 'large' })` returned on Windows.
pub fn large_icon_size() -> u32 {
    (unsafe { GetSystemMetrics(SM_CXICON) }).max(16) as u32
}

/// Auto-detected app icons: ask the shell for its biggest image, not the
/// 32px large icon, so the saved PNG keeps the exe's full detail.
pub const EXTRACT_ICON_PX: u32 = 256;

impl Wic {
    pub fn new() -> Result<Wic> {
        let com = Com::init();
        let f = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ctx("WIC isn't available")?;
        Ok(Wic { f, _com: com })
    }

    fn decoder(&self, bytes: &[u8]) -> Result<IWICBitmapDecoder> {
        // SHCreateMemStream copies the (compressed) bytes, so the decoder never
        // points into a caller's buffer.
        let stream = unsafe { SHCreateMemStream(Some(bytes)) }.ok_or_else(|| Error::msg("Out of memory"))?;
        unsafe { self.f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand) }
            .ctx("Not an image Windows can read")
    }

    /// Frames to try: for an .ico, its images largest first (core's
    /// `ico_entries` order, whose indexes are WIC's frame numbers); else the first.
    fn frames(bytes: &[u8]) -> Vec<u32> {
        let ico = ico_entries(bytes);
        if ico.is_empty() {
            vec![0]
        } else {
            ico.iter().map(|e| e.index as u32).collect()
        }
    }

    /// PNG, JPEG, ICO (best frame) or anything else WIC reads, not yet decoded.
    pub fn decode(&self, bytes: &[u8]) -> Result<Source> {
        let dec = self.decoder(bytes)?;
        let mut last = Error::msg("The image has no frames");
        for i in Self::frames(bytes) {
            match unsafe { dec.GetFrame(i) }.ctx("Couldn't read the image") {
                Ok(frame) => return Ok(Source(frame.cast().ctx("WIC")?)),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Decodes into memory at the size `fit(width, height)` picks, trying an
    /// .ico's frames in order until one fully decodes.
    pub fn decode_rgba(&self, bytes: &[u8], fit: impl Fn(u32, u32) -> (u32, u32)) -> Result<Rgba> {
        let dec = self.decoder(bytes)?;
        let mut last = Error::msg("The image has no frames");
        for i in Self::frames(bytes) {
            let attempt = || -> Result<Rgba> {
                let src = Source(unsafe { dec.GetFrame(i) }.ctx("Couldn't read the image")?.cast().ctx("WIC")?);
                let (w, h) = self.size(&src);
                let (tw, th) = fit(w, h);
                self.to_rgba(&self.resize(&src, tw, th)?)
            };
            match attempt() {
                Ok(img) => return Ok(img),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub fn size(&self, src: &Source) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        let _ = unsafe { src.0.GetSize(&mut w, &mut h) };
        (w, h)
    }

    /// Scaled to exactly `width` x `height` (high-quality cubic); the same
    /// source back if it already is that size.
    pub fn resize(&self, src: &Source, width: u32, height: u32) -> Result<Source> {
        if self.size(src) == (width, height) {
            return Ok(Source(src.0.clone()));
        }
        let scaler = unsafe { self.f.CreateBitmapScaler() }.ctx("WIC scaler")?;
        unsafe { scaler.Initialize(&src.0, width.max(1), height.max(1), WICBitmapInterpolationModeHighQualityCubic) }
            .ctx("Couldn't scale the image")?;
        Ok(Source(scaler.cast().ctx("WIC")?))
    }

    fn convert(&self, src: &IWICBitmapSource, format: &GUID) -> Result<IWICBitmapSource> {
        if unsafe { src.GetPixelFormat() }.ok().as_ref() == Some(format) {
            return Ok(src.clone());
        }
        let conv = unsafe { self.f.CreateFormatConverter() }.ctx("WIC converter")?;
        unsafe { conv.Initialize(src, format, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom) }
            .ctx("Couldn't convert the image")?;
        conv.cast().ctx("WIC")
    }

    fn pixels(&self, src: &IWICBitmapSource, format: &GUID) -> Result<(u32, u32, Vec<u8>)> {
        let src = self.convert(src, format)?;
        let (mut w, mut h) = (0, 0);
        unsafe { src.GetSize(&mut w, &mut h) }.ctx("WIC")?;
        let mut buf = vec![0u8; w as usize * h as usize * 4];
        unsafe { src.CopyPixels(std::ptr::null(), w * 4, &mut buf) }.ctx("Couldn't decode the image")?;
        Ok((w, h, buf))
    }

    pub fn to_rgba(&self, src: &Source) -> Result<Rgba> {
        let (width, height, rgba) = self.pixels(&src.0, &GUID_WICPixelFormat32bppRGBA)?;
        Ok(Rgba { width, height, rgba })
    }

    /// An in-memory image as a source (copies the pixels into a WIC bitmap).
    pub fn rgba_source(&self, img: &Rgba) -> Result<Source> {
        let bmp = unsafe {
            self.f.CreateBitmapFromMemory(img.width, img.height, &GUID_WICPixelFormat32bppRGBA, img.width * 4, &img.rgba)
        }
        .ctx("WIC bitmap")?;
        Ok(Source(bmp.cast().ctx("WIC")?))
    }

    /// PNG (32-bit, alpha kept) or JPEG (alpha dropped) bytes.
    pub fn encode(&self, src: &Source, format: Format) -> Result<Vec<u8>> {
        let (container, mut pixel) = match format {
            Format::Png => (GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA),
            Format::Jpeg(_) => (GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR),
        };
        let stream: IStream = unsafe { SHCreateMemStream(None) }.ok_or_else(|| Error::msg("Out of memory"))?;
        let enc = unsafe { self.f.CreateEncoder(&container, std::ptr::null()) }.ctx("WIC encoder")?;
        unsafe { enc.Initialize(&stream, WICBitmapEncoderNoCache) }.ctx("WIC encoder")?;
        let (mut frame, mut bag) = (None, None);
        unsafe { enc.CreateNewFrame(&mut frame, &mut bag) }.ctx("WIC encoder")?;
        let (Some(frame), Some(bag)) = (frame, bag) else { return Err(Error::msg("WIC encoder gave no frame")) };
        if let Format::Jpeg(q) = format {
            set_quality(&bag, f32::from(q.min(100)) / 100.0)?;
        }
        let (w, h) = self.size(src);
        unsafe {
            frame.Initialize(&bag).ctx("WIC encoder")?;
            frame.SetSize(w, h).ctx("WIC encoder")?;
            // The encoder may pick a different format; convert to whatever it chose.
            frame.SetPixelFormat(&mut pixel).ctx("WIC encoder")?;
        }
        let input = self.convert(&src.0, &pixel)?;
        unsafe {
            frame.WriteSource(&input, std::ptr::null()).ctx("Couldn't encode the image")?;
            frame.Commit().ctx("Couldn't encode the image")?;
            enc.Commit().ctx("Couldn't encode the image")?;
        }
        read_stream(&stream)
    }

    pub fn encode_rgba(&self, img: &Rgba, format: Format) -> Result<Vec<u8>> {
        self.encode(&self.rgba_source(img)?, format)
    }

    /// The shell's image for a path, from IShellItemImageFactory.
    fn shell_image(&self, path: &Path, size: u32, flags: SIIGBF) -> Result<Rgba> {
        let factory: IShellItemImageFactory =
            unsafe { SHCreateItemFromParsingName(PCWSTR(wide(path).as_ptr()), None) }.ctx("The shell can't find that path")?;
        let side = size.max(1) as i32;
        let hbmp = unsafe { factory.GetImage(SIZE { cx: side, cy: side }, flags) }.ctx("No image for that file")?;
        let bmp = unsafe { self.f.CreateBitmapFromHBITMAP(hbmp, HPALETTE::default(), WICBitmapUsePremultipliedAlpha) };
        unsafe {
            let _ = DeleteObject(hbmp.into());
        }
        let src: IWICBitmapSource = bmp.ctx("WIC bitmap")?.cast().ctx("WIC")?;
        let (width, height, mut px) = self.pixels(&src, &GUID_WICPixelFormat32bppPBGRA)?;
        // Thumbnails of opaque images often come with every alpha byte 0.
        if px.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            px.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
        }
        for p in px.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            let un = |c: u8| (u32::from(c) * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
            let (b, g, r) = (un(p[0]), un(p[1]), un(p[2]));
            p[0] = r;
            p[1] = g;
            p[2] = b;
        }
        Ok(Rgba { width, height, rgba: px })
    }

    /// A file's, folder's or exe's icon at `size` px (IShellItemImageFactory,
    /// icon only), falling back to SHGetFileInfo's large icon scaled to `size`.
    /// An exe with only small icons comes back as a small picture centered on
    /// a big empty square; that picture is cut out and scaled up to fill it.
    pub fn file_icon(&self, path: &Path, size: u32) -> Result<Rgba> {
        let img = self.shell_image(path, size, SIIGBF_ICONONLY).or_else(|e| self.file_info_icon(path, size).map_err(|_| e))?;
        match small_icon_box(&img) {
            Some(b) => self.fill(&img, b, size),
            None => Ok(img),
        }
    }

    /// The `side`-px square at (x, y) of `img`, scaled to `size` (premultiplied,
    /// so the transparent edge doesn't turn into a dark fringe).
    fn fill(&self, img: &Rgba, (x, y, side): (u32, u32, u32), size: u32) -> Result<Rgba> {
        let mut px = Vec::with_capacity((side * side * 4) as usize);
        for row in y..y + side {
            let start = ((row * img.width + x) * 4) as usize;
            for p in img.rgba[start..start + (side * 4) as usize].as_chunks::<4>().0 {
                let a = u32::from(p[3]);
                px.extend_from_slice(&[(u32::from(p[0]) * a / 255) as u8, (u32::from(p[1]) * a / 255) as u8, (u32::from(p[2]) * a / 255) as u8, p[3]]);
            }
        }
        let bmp = unsafe { self.f.CreateBitmapFromMemory(side, side, &GUID_WICPixelFormat32bppPRGBA, side * 4, &px) }.ctx("WIC bitmap")?;
        let src = Source(bmp.cast().ctx("WIC")?);
        self.to_rgba(&self.resize(&src, size, size)?)
    }

    fn file_info_icon(&self, path: &Path, size: u32) -> Result<Rgba> {
        let mut info = SHFILEINFOW::default();
        let ok = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide(path).as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        if ok == 0 || info.hIcon.is_invalid() {
            return Err(Error::msg("No icon for that file"));
        }
        let bmp = unsafe { self.f.CreateBitmapFromHICON(info.hIcon) };
        unsafe {
            let _ = DestroyIcon(info.hIcon);
        }
        let src = Source(bmp.ctx("WIC bitmap")?.cast().ctx("WIC")?);
        self.to_rgba(&self.resize(&src, size, size)?)
    }

    /// The shell's thumbnail (images, PDFs, videos) fitting in `size` px;
    /// an error for a type with no thumbnail handler.
    pub fn thumbnail(&self, path: &Path, size: u32) -> Result<Rgba> {
        self.shell_image(path, size, SIIGBF_THUMBNAILONLY)
    }

    /// A picked image file as a 128x128 PNG: Electron's
    /// `createFromPath(p).resize({ width: 128, height: 128 })`, which
    /// stretches a non-square image.
    pub fn custom_icon_png(&self, path: &Path) -> Result<Vec<u8>> {
        let bytes = std::fs::read(path).map_err(|_| Error::msg("Could not read image file"))?;
        let img = self.decode_rgba(&bytes, |_, _| (128, 128)).map_err(|_| Error::msg("Could not read image file"))?;
        self.encode_rgba(&img, Format::Png)
    }
}

/// The square (x, y, side) around `img`'s visible pixels, when they take up
/// at most half its width: a small icon drawn in the middle of a big square.
fn small_icon_box(img: &Rgba) -> Option<(u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, p) in img.rgba.as_chunks::<4>().0.iter().enumerate() {
        // Mostly opaque only: the shell draws a faint frame around the whole square.
        if p[3] >= 128 {
            let (x, y) = (i as u32 % img.width, i as u32 / img.width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        }
    }
    // Two pixels of room for the icon's soft edge.
    let (x0, y0, x1, y1) = (x0.saturating_sub(2), y0.saturating_sub(2), x1.saturating_add(2), y1.saturating_add(2));
    let side = x1.checked_sub(x0)?.max(y1.checked_sub(y0)?);
    let room = img.width.min(img.height);
    if side == 0 || side * 2 > room {
        return None;
    }
    let at = |lo: u32, hi: u32| ((lo + hi) / 2).saturating_sub(side / 2).min(room - side);
    Some((at(x0, x1), at(y0, y1), side))
}

fn set_quality(bag: &IPropertyBag2, q: f32) -> Result<()> {
    let mut name = wide("ImageQuality");
    let prop = PROPBAG2 { pstrName: windows::core::PWSTR(name.as_mut_ptr()), ..Default::default() };
    let value = VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_R4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { fltVal: q },
            }),
        },
    };
    // A VT_R4 VARIANT owns nothing, so not clearing it leaks nothing.
    unsafe { bag.Write(1, &prop, &value) }.ctx("JPEG quality")
}

fn read_stream(stream: &IStream) -> Result<Vec<u8>> {
    let mut len = 0u64;
    unsafe {
        stream.Seek(0, STREAM_SEEK_END, Some(&mut len)).ctx("Stream")?;
        stream.Seek(0, STREAM_SEEK_SET, None).ctx("Stream")?;
    }
    let mut out = vec![0u8; len as usize];
    let mut read = 0u32;
    unsafe { stream.Read(out.as_mut_ptr().cast(), out.len() as u32, Some(&mut read)) }.ok().ctx("Stream")?;
    out.truncate(read as usize);
    Ok(out)
}

fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str> {
    arg(args, name).ok_or_else(|| Error::msg(format!("{name} is required")))
}

fn num(args: &[String], name: &str) -> Result<u32> {
    need(args, name)?.parse().map_err(|_| Error::msg(format!("{name} must be a number")))
}

/// `icon --path P [--size N] --out f.png`, `thumb --path P --out f.png`,
/// `resize --in a.jpg --w N --h N --out b.jpg` (b's extension picks PNG or JPEG),
/// `custom --in P --out f.png` (a picked custom icon, 128x128).
pub fn probe(args: &[String]) -> Result<Value> {
    let wic = Wic::new()?;
    let (img, out) = match args.first().map(String::as_str) {
        Some("icon") => {
            let size = if arg(args, "--size").is_some() { num(args, "--size")? } else { large_icon_size() };
            (wic.file_icon(Path::new(need(args, "--path")?), size)?, need(args, "--out")?)
        }
        Some("thumb") => (wic.thumbnail(Path::new(need(args, "--path")?), launchpad_core::file_store::THUMB_SIZE)?, need(args, "--out")?),
        Some("resize") => {
            let out = need(args, "--out")?;
            let bytes = std::fs::read(need(args, "--in")?)?;
            let src = wic.decode(&bytes)?;
            let before = wic.size(&src);
            let resized = wic.resize(&src, num(args, "--w")?, num(args, "--h")?)?;
            let jpeg = !out.to_ascii_lowercase().ends_with(".png");
            let format = if jpeg { Format::Jpeg(launchpad_core::media::art::JPEG_QUALITY) } else { Format::Png };
            let data = wic.encode(&resized, format)?;
            std::fs::write(out, &data)?;
            let after = wic.size(&resized);
            return Ok(json!({ "from": [before.0, before.1], "to": [after.0, after.1], "bytes": data.len(), "out": out }));
        }
        Some("custom") => {
            let png = wic.custom_icon_png(Path::new(need(args, "--in")?))?;
            let out = need(args, "--out")?;
            std::fs::write(out, &png)?;
            return Ok(json!({ "bytes": png.len(), "out": out }));
        }
        _ => return Err(Error::msg("Usage: --probe image icon|thumb|resize|custom …")),
    };
    let png = wic.encode_rgba(&img, Format::Png)?;
    std::fs::write(out, &png)?;
    let opaque = img.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] == 255).count();
    Ok(json!({ "width": img.width, "height": img.height, "opaquePixels": opaque, "bytes": png.len(), "out": out }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Rgba {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(if (x + y) % 2 == 0 { &[255, 0, 0, 255] } else { &[0, 0, 255, 128] });
            }
        }
        Rgba { width: w, height: h, rgba }
    }

    #[test]
    fn png_round_trips_straight_alpha() {
        let wic = Wic::new().unwrap();
        let img = checker(5, 3);
        let png = wic.encode_rgba(&img, Format::Png).unwrap();
        assert!(launchpad_core::favicon::is_png(&png));
        assert_eq!(wic.decode_rgba(&png, |w, h| (w, h)).unwrap(), img);
    }

    #[test]
    fn jpeg_encodes_and_resizes() {
        let wic = Wic::new().unwrap();
        let jpg = wic.encode_rgba(&checker(40, 20), Format::Jpeg(88)).unwrap();
        assert!(launchpad_core::favicon::is_jpeg(&jpg));
        let src = wic.decode(&jpg).unwrap();
        assert_eq!(wic.size(&src), (40, 20));
        assert_eq!(wic.size(&wic.resize(&src, 10, 5).unwrap()), (10, 5));
    }

    #[test]
    fn ico_uses_the_largest_frame_that_decodes() {
        let wic = Wic::new().unwrap();
        let small = wic.encode_rgba(&checker(16, 16), Format::Png).unwrap();
        let big = wic.encode_rgba(&checker(48, 48), Format::Png).unwrap();
        // Directory: 16 px (good), 64 px (garbage), 48 px (good).
        let junk = vec![7u8; 40];
        let entries: [(u8, &[u8]); 3] = [(16, &small), (64, &junk), (48, &big)];
        let mut ico = vec![0, 0, 1, 0, 3, 0];
        let mut offset = 6 + 16 * entries.len() as u32;
        for (w, data) in entries {
            ico.extend_from_slice(&[w, w, 0, 0, 1, 0, 32, 0]);
            ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
            ico.extend_from_slice(&offset.to_le_bytes());
            offset += data.len() as u32;
        }
        for (_, data) in entries {
            ico.extend_from_slice(data);
        }
        let img = wic.decode_rgba(&ico, |w, h| (w, h)).unwrap();
        assert_eq!((img.width, img.height), (48, 48));
    }

    #[test]
    fn small_icons_are_cut_out_and_scaled_up() {
        let mut img = Rgba { width: 256, height: 256, rgba: vec![0; 256 * 256 * 4] };
        for y in 112..144 {
            for x in 112..144 {
                let i = ((y * 256 + x) * 4) as usize;
                img.rgba[i..i + 4].copy_from_slice(&[200, 10, 10, 255]);
            }
        }
        assert_eq!(small_icon_box(&img), Some((110, 110, 36)));
        // The faint frame the shell draws round the square doesn't count.
        img.rgba[3] = 40;
        assert_eq!(small_icon_box(&img), Some((110, 110, 36)));
        let wic = Wic::new().unwrap();
        let big = wic.fill(&img, (112, 112, 32), 256).unwrap();
        assert_eq!((big.width, big.height), (256, 256));
        assert_eq!(&big.rgba[..4], &[200, 10, 10, 255]);
        // A full-size icon, or nothing at all, is left as it is.
        assert_eq!(small_icon_box(&checker(64, 64)), None);
        assert_eq!(small_icon_box(&Rgba { width: 8, height: 8, rgba: vec![0; 256] }), None);
    }

    #[test]
    fn rejects_non_images() {
        let wic = Wic::new().unwrap();
        assert!(wic.decode(b"<html>nope</html>").is_err());
        assert!(wic.decode_rgba(b"", |w, h| (w, h)).is_err());
    }
}
