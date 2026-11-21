//! Removable USB drives. Replaces lib\drives.ps1 (Get-Volume / Get-Disk
//! BusType, Shell "Eject" verb).
//!
//! Changes arrive as WM_DEVICECHANGE: see `drives_may_have_changed`.
use super::{from_wide, require_yes, wide, Com, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVTYP_VOLUME, DEV_BROADCAST_HDR, SW_SHOWNORMAL,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    /// "E:"
    pub drive_letter: String,
    pub label: String,
    /// Bytes.
    pub size: u64,
    pub free_space: u64,
}

impl Drive {
    pub fn to_json(&self) -> Value {
        json!({ "driveLetter": self.drive_letter, "label": self.label, "size": self.size, "freeSpace": self.free_space })
    }
}

/// The volume's disk is on the USB bus (flash drives and external disks; the .ps1's `BusType -eq 'USB'`).
fn is_usb(letter: char) -> bool {
    let Ok(h) = (unsafe {
        // No access rights needed for a property query, so no elevation either.
        CreateFileW(
            PCWSTR(wide(format!(r"\\.\{letter}:")).as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }) else {
        return false;
    };
    let query = STORAGE_PROPERTY_QUERY { PropertyId: StorageDeviceProperty, QueryType: PropertyStandardQuery, ..Default::default() };
    let mut desc = STORAGE_DEVICE_DESCRIPTOR::default(); // the fixed part is enough; the driver truncates the rest
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            std::mem::size_of_val(&query) as u32,
            Some(&mut desc as *mut _ as *mut _),
            std::mem::size_of_val(&desc) as u32,
            None,
            None,
        )
    }
    .is_ok();
    let _ = unsafe { CloseHandle(h) };
    ok && desc.BusType == BusTypeUsb
}

/// USB drives with a letter and a mounted volume, in letter order.
pub fn list() -> Result<Vec<Drive>> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for (i, letter) in ('A'..='Z').enumerate() {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = wide(format!(r"{letter}:\"));
        // 2 = DRIVE_REMOVABLE, 3 = DRIVE_FIXED; optical, network and RAM drives never had a partition for the .ps1.
        if !matches!(unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }, 2 | 3) || !is_usb(letter) {
            continue;
        }
        let (mut size, mut free) = (0u64, 0u64);
        // Fails for an empty card reader: no volume, so the .ps1 skipped it too.
        if unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), None, Some(&mut size), Some(&mut free)) }.is_err() {
            continue;
        }
        let mut label = [0u16; 261];
        let _ = unsafe { GetVolumeInformationW(PCWSTR(root.as_ptr()), Some(&mut label), None, None, None, None) };
        out.push(Drive { drive_letter: format!("{letter}:"), label: from_wide(&label), size, free_space: free });
    }
    Ok(out)
}

/// "E", "E:" or "e:\" → 'E'.
fn parse_letter(s: &str) -> Option<char> {
    let mut c = s.chars();
    let l = c.next()?.to_ascii_uppercase();
    let rest = c.as_str();
    (l.is_ascii_uppercase() && matches!(rest, "" | ":" | r":\")).then_some(l)
}

/// Explorer's right-click > Eject on the drive, as the .ps1 did. For a USB
/// stick that also safely removes it; for an external disk Explorer does a
/// safe-remove of the device. Windows shows its own "can't eject" message if
/// files are open. Returns when the verb has run; the drive's removal then
/// arrives as WM_DEVICECHANGE.
pub fn eject(letter: &str) -> Result<()> {
    let l = parse_letter(letter).ok_or_else(|| Error::msg(format!("{letter:?} isn't a drive letter.")))?;
    let _com = Com::init();
    let file = wide(format!(r"{l}:\"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOASYNC,
        lpVerb: w!("eject"),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.ctx("Eject")
}

/// For Phase 3's window procedure on WM_DEVICECHANGE: true when a volume
/// arrived or left, i.e. call `list()` again. A top-level window (hidden is
/// fine, HWND_MESSAGE is not) gets these broadcasts without registering.
///
/// # Safety
/// `wparam` and `lparam` must be the ones of a WM_DEVICECHANGE message.
#[allow(dead_code)] // called by Phase 3's window procedure
pub unsafe fn drives_may_have_changed(wparam: WPARAM, lparam: LPARAM) -> bool {
    matches!(wparam.0 as u32, DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE)
        && lparam.0 != 0
        // Safety: for these two events lparam points at a DEV_BROADCAST_HDR.
        && unsafe { (*(lparam.0 as *const DEV_BROADCAST_HDR)).dbch_devicetype } == DBT_DEVTYP_VOLUME
}

pub fn probe(args: &[String]) -> Result<Value> {
    match args.first().map(String::as_str) {
        None | Some("list") => Ok(Value::Array(list()?.iter().map(Drive::to_json).collect())),
        Some("eject") => {
            let letter = super::arg(args, "--letter").ok_or_else(|| Error::msg("eject needs --letter <E>."))?;
            parse_letter(letter).ok_or_else(|| Error::msg(format!("{letter:?} isn't a drive letter.")))?;
            require_yes(args, "Ejecting a drive")?;
            eject(letter)?;
            Ok(json!({ "ok": true }))
        }
        Some(other) => Err(Error::msg(format!("No drives action {other}. Try list, eject."))),
    }
}

/// The letters of the fixed (internal) disks, "C", "D"…: safe to look at
/// without waiting on a network share or a sleeping USB drive.
pub fn fixed_letters() -> Vec<char> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i) as char)
        .filter(|l| unsafe { GetDriveTypeW(PCWSTR(wide(format!("{l}:\\")).as_ptr())) } == 3) // DRIVE_FIXED
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{DBT_DEVTYP_PORT, DEV_BROADCAST_VOLUME};

    #[test]
    fn parses_letters() {
        assert_eq!(parse_letter("E"), Some('E'));
        assert_eq!(parse_letter("e:"), Some('E'));
        assert_eq!(parse_letter(r"F:\"), Some('F'));
        for bad in ["", "EE", "1:", "E:/x", "É"] {
            assert_eq!(parse_letter(bad), None, "{bad}");
        }
    }

    #[test]
    fn device_change_filter() {
        let mut v = DEV_BROADCAST_VOLUME { dbcv_size: 20, dbcv_devicetype: DBT_DEVTYP_VOLUME.0, dbcv_unitmask: 1 << 4, ..Default::default() };
        let p = LPARAM(&mut v as *mut _ as isize);
        unsafe {
            assert!(drives_may_have_changed(WPARAM(DBT_DEVICEARRIVAL as usize), p));
            assert!(drives_may_have_changed(WPARAM(DBT_DEVICEREMOVECOMPLETE as usize), p));
            assert!(!drives_may_have_changed(WPARAM(0x0007), LPARAM(0))); // DBT_DEVNODES_CHANGED
            v.dbcv_devicetype = DBT_DEVTYP_PORT.0;
            assert!(!drives_may_have_changed(WPARAM(DBT_DEVICEARRIVAL as usize), LPARAM(&mut v as *mut _ as isize)));
        }
    }

    #[test]
    fn eject_checks_its_arguments() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(probe(&a(&["eject", "--letter", "E"])).unwrap_err().to_string().contains("--yes"));
        assert!(probe(&a(&["eject", "--letter", "EE", "--yes"])).unwrap_err().to_string().contains("drive letter"));
        assert!(probe(&a(&["eject", "--yes"])).unwrap_err().to_string().contains("--letter"));
    }
}
