//! Monitors, their work areas and scaling, in physical pixels (replaces
//! Electron's `screen.*`; the process is per-monitor DPI aware v2, so no
//! DIP conversion happens anywhere).
use super::from_wide;
use launchpad_core::Result;
use serde_json::{json, Value};
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, MONITORINFOF_PRIMARY, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// Stable while the display stays connected to the same output: a hash of
    /// its device name (`\\.\DISPLAY1`). Electron's ids differ, so scenes
    /// saved by 1.2.2 fall back to matching by bounds (`scenes::pick_display`).
    pub id: u32,
    pub device: String,
    pub bounds: RECT,
    pub work: RECT,
    /// Effective DPI / 96: 1.0 at 100 %, 1.5 at 150 %.
    pub scale: f64,
    pub primary: bool,
}

/// Every monitor, in EnumDisplayMonitors order.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn on_monitor(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        // lp is the &mut Vec passed below, alive for the whole enumeration.
        let list = unsafe { &mut *(lp.0 as *mut Vec<HMONITOR>) };
        list.push(m);
        true.into()
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(on_monitor), LPARAM(&mut handles as *mut _ as isize));
    }
    handles.into_iter().filter_map(info).collect()
}

/// One monitor's details, or None if it went away meanwhile.
pub fn info(m: HMONITOR) -> Option<Monitor> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO) }.as_bool() {
        return None;
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
    let device = from_wide(&mi.szDevice);
    Some(Monitor {
        id: fnv1a(&device),
        device,
        bounds: mi.monitorInfo.rcMonitor,
        work: mi.monitorInfo.rcWork,
        scale: dx as f64 / 96.0,
        primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

/// The primary monitor's work area (SPI_GETWORKAREA).
pub fn work_area() -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    r
}

fn fnv1a(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// A rect as `[left, top, right, bottom]`, the form the old helper replied with.
pub fn ltrb(r: &RECT) -> Value {
    json!([r.left, r.top, r.right, r.bottom])
}

pub fn monitor_json(m: &Monitor) -> Value {
    json!({ "id": m.id, "device": m.device, "bounds": ltrb(&m.bounds), "work": ltrb(&m.work), "scale": m.scale, "primary": m.primary })
}

pub fn probe(_args: &[String]) -> Result<Value> {
    Ok(json!({
        "monitors": monitors().iter().map(monitor_json).collect::<Vec<_>>(),
        "workArea": ltrb(&work_area()),
    }))
}

/// The primary display's scale (1.0 at 96 DPI).
pub fn primary_scale() -> f64 {
    monitors().iter().find(|m| m.primary).map_or(1.0, |m| m.scale)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_stable_and_differ_by_device() {
        assert_eq!(super::fnv1a(r"\\.\DISPLAY1"), super::fnv1a(r"\\.\DISPLAY1"));
        assert_ne!(super::fnv1a(r"\\.\DISPLAY1"), super::fnv1a(r"\\.\DISPLAY2"));
    }
}
