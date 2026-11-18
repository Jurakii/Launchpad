//! Test tooling for the UI phases: a picture of one window, and mouse/key
//! input posted to it, so a test can drive Launchpad's own windows without
//! moving the person's real cursor or taking the foreground.
//!
//! `--probe capture --hwnd N --out shot.png`
//! `--probe capture click --hwnd N --x X --y Y [--right] [--double]`
//! `--probe capture move --hwnd N --x X --y Y`   (hover)
//! `--probe capture drag --hwnd N --x X --y Y --to-x X2 --to-y Y2`
//! `--probe capture wheel --hwnd N --x X --y Y --delta -120`
//! `--probe capture type --hwnd N --text "abc"`
//! `--probe capture key --hwnd N --vk 0x0D`      (Enter; 0x1B Esc, 0x09 Tab…)
//! Coordinates are client-area physical pixels, as in the capture.
use super::image::{Format, Rgba, Wic};
use super::{arg, wide, windowmgr, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::thread::sleep;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::*;

const PW_RENDERFULLCONTENT: u32 = 2;

fn lparam(x: i32, y: i32) -> LPARAM {
    LPARAM(((y as u16 as isize) << 16) | x as u16 as isize)
}

fn post(h: HWND, msg: u32, w: usize, l: LPARAM) {
    unsafe {
        let _ = PostMessageW(Some(h), msg, WPARAM(w), l);
    }
    sleep(Duration::from_millis(30));
}

/// The client area as top-down RGBA.
pub fn capture(h: HWND) -> Result<Rgba> {
    let mut r = RECT::default();
    unsafe { GetClientRect(h, &mut r) }.ctx("GetClientRect")?;
    let (w, ht) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || ht <= 0 {
        return Err(Error::msg("The window has no client area (minimized or hidden?)."));
    }
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, ht);
        let old = SelectObject(dc, bmp.into());
        let ok = PrintWindow(h, dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT | 1)).as_bool(); // | PW_CLIENTONLY
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -ht,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * ht * 4) as usize];
        GetDIBits(dc, bmp, 0, ht as u32, Some(px.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        if !ok {
            return Err(Error::msg("PrintWindow failed."));
        }
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2); // BGRA -> RGBA
            p[3] = 255;
        }
        Ok(Rgba { width: w as u32, height: ht as u32, rgba: px })
    }
}

fn num(args: &[String], name: &str) -> Result<i32> {
    let v = arg(args, name).ok_or_else(|| Error::msg(format!("{name} is required")))?;
    let v = v.trim();
    match v.strip_prefix("0x") {
        Some(hex) => i32::from_str_radix(hex, 16),
        None => v.parse(),
    }
    .map_err(|_| Error::msg(format!("{name} must be a number")))
}

pub fn probe(args: &[String]) -> Result<Value> {
    let action = args.first().filter(|a| !a.starts_with("--")).map(String::as_str).unwrap_or("shot");
    let h = windowmgr::hwnd(num(args, "--hwnd")? as i64);
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        return Err(Error::msg("No such window."));
    }
    let at = |xn: &str, yn: &str| -> Result<LPARAM> { Ok(lparam(num(args, xn)?, num(args, yn)?)) };
    match action {
        "shot" => {
            let out = arg(args, "--out").ok_or_else(|| Error::msg("--out is required"))?;
            let img = capture(h)?;
            let png = Wic::new()?.encode_rgba(&img, Format::Png)?;
            std::fs::write(out, png).map_err(|e| Error::msg(format!("Writing {out}: {e}")))?;
            Ok(json!({ "out": out, "width": img.width, "height": img.height }))
        }
        "move" => {
            post(h, WM_MOUSEMOVE, 0, at("--x", "--y")?);
            Ok(json!({ "ok": true }))
        }
        "click" => {
            let l = at("--x", "--y")?;
            let right = args.iter().any(|a| a == "--right");
            let (down, up, mk) = if right { (WM_RBUTTONDOWN, WM_RBUTTONUP, 2) } else { (WM_LBUTTONDOWN, WM_LBUTTONUP, 1) };
            post(h, WM_MOUSEMOVE, 0, l);
            let times = if args.iter().any(|a| a == "--double") { 2 } else { 1 };
            for _ in 0..times {
                post(h, down, mk, l);
                post(h, up, 0, l);
            }
            Ok(json!({ "ok": true }))
        }
        "drag" => {
            let (x0, y0, x1, y1) = (num(args, "--x")?, num(args, "--y")?, num(args, "--to-x")?, num(args, "--to-y")?);
            post(h, WM_MOUSEMOVE, 0, lparam(x0, y0));
            post(h, WM_LBUTTONDOWN, 1, lparam(x0, y0));
            for i in 1..=10 {
                post(h, WM_MOUSEMOVE, 1, lparam(x0 + (x1 - x0) * i / 10, y0 + (y1 - y0) * i / 10));
            }
            post(h, WM_LBUTTONUP, 0, lparam(x1, y1));
            Ok(json!({ "ok": true }))
        }
        "wheel" => {
            // WM_MOUSEWHEEL carries screen coordinates.
            let mut p = windows::Win32::Foundation::POINT { x: num(args, "--x")?, y: num(args, "--y")? };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(h, &mut p);
            }
            let delta = num(args, "--delta")?;
            post(h, WM_MOUSEWHEEL, (delta as i16 as u16 as usize) << 16, lparam(p.x, p.y));
            Ok(json!({ "ok": true }))
        }
        "type" => {
            let text = arg(args, "--text").ok_or_else(|| Error::msg("--text is required"))?;
            for u in wide(text).into_iter().take_while(|&c| c != 0) {
                post(h, WM_CHAR, u as usize, LPARAM(1));
            }
            Ok(json!({ "ok": true }))
        }
        "key" => {
            let vk = num(args, "--vk")? as usize;
            post(h, WM_KEYDOWN, vk, LPARAM(1));
            post(h, WM_KEYUP, vk, LPARAM(1 | (3 << 30)));
            Ok(json!({ "ok": true }))
        }
        other => Err(Error::msg(format!("Unknown capture action {other}."))),
    }
}
