//! Paired Bluetooth devices through SetupAPI / CfgMgr32. Replaces
//! lib\bluetooth.ps1 (Get-PnpDevice / Disable-PnpDevice).
use super::{arg, from_wide, require_yes, wide};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::Properties::{DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName, DEVPROPTYPE};
use windows::Win32::Foundation::DEVPROPKEY;

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// Friendly name, else the device description (what Get-PnpDevice shows).
    pub name: Option<String>,
    pub instance_id: String,
    /// The .ps1's `Status -eq 'OK'`.
    pub connected: bool,
}

/// Get-PnpDevice's Status "OK": no problem and not "Degraded" (0x1000,
/// DN_LEGACY_DRIVER). Started or not doesn't matter. Checked against all
/// 241 devices on the dev PC.
fn is_status_ok(s: CM_DEVNODE_STATUS_FLAGS) -> bool {
    s.0 & (DN_HAS_PROBLEM.0 | DN_PRIVATE_PROBLEM.0 | DN_LEGACY_DRIVER.0) == 0
}

impl Device {
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "instanceId": self.instance_id, "connected": self.connected })
    }
}

/// BTHENUM/BTHLE instance ids are the paired peripherals; the radio itself and
/// Microsoft's enumerator entries live elsewhere (the .ps1's filter, case-insensitive like `-like`).
fn is_bluetooth_device(instance_id: &str) -> bool {
    let up = instance_id.to_ascii_uppercase();
    up.starts_with("BTHENUM") || up.starts_with("BTHLE")
}

struct DevList(HDEVINFO);

impl Drop for DevList {
    fn drop(&mut self) {
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

fn string_prop(list: &DevList, d: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
    let (mut ty, mut buf) = (DEVPROPTYPE::default(), [0u8; 1024]);
    unsafe { SetupDiGetDevicePropertyW(list.0, d, key, &mut ty, Some(&mut buf), None, 0) }.ok()?;
    let w: Vec<u16> = buf.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    Some(from_wide(&w)).filter(|s| !s.is_empty())
}

/// Present BTHENUM*/BTHLE* devices.
pub fn list() -> Result<Vec<Device>> {
    let list = DevList(
        unsafe { SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT) }
            .map_err(|e| Error::msg(format!("SetupDiGetClassDevs: {}", e.message())))?,
    );
    let mut out = Vec::new();
    for i in 0.. {
        let mut d = SP_DEVINFO_DATA { cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
        if unsafe { SetupDiEnumDeviceInfo(list.0, i, &mut d) }.is_err() {
            break; // ERROR_NO_MORE_ITEMS
        }
        let mut id = [0u16; 512]; // MAX_DEVICE_ID_LEN is 200
        if unsafe { SetupDiGetDeviceInstanceIdW(list.0, &d, Some(&mut id), None) }.is_err() {
            continue;
        }
        let instance_id = from_wide(&id);
        if !is_bluetooth_device(&instance_id) {
            continue;
        }
        let (mut status, mut problem) = (CM_DEVNODE_STATUS_FLAGS::default(), CM_PROB::default());
        let ok = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, d.DevInst, 0) } == CR_SUCCESS;
        out.push(Device {
            name: string_prop(&list, &d, &DEVPKEY_Device_FriendlyName).or_else(|| string_prop(&list, &d, &DEVPKEY_Device_DeviceDesc)),
            instance_id,
            connected: ok && is_status_ok(status),
        });
    }
    Ok(out)
}

const CR_ACCESS_DENIED: u32 = 0x33;

/// "Disconnect" as the .ps1 did it: disable the device (persistently, like
/// Device Manager). Needs an elevated process: unelevated it fails with
/// access denied, exactly as Disable-PnpDevice did. Nothing re-enables it.
pub fn disconnect(instance_id: &str) -> Result<()> {
    if !is_bluetooth_device(instance_id) {
        return Err(Error::msg("Only BTHENUM / BTHLE devices can be disconnected."));
    }
    let mut inst = 0;
    let r = unsafe { CM_Locate_DevNodeW(&mut inst, PCWSTR(wide(instance_id).as_ptr()), CM_LOCATE_DEVNODE_NORMAL) };
    if r != CR_SUCCESS {
        return Err(Error::msg(format!("No device {instance_id} (CONFIGRET {}).", r.0)));
    }
    match unsafe { CM_Disable_DevNode(inst, CM_DISABLE_PERSIST) }.0 {
        0 => Ok(()),
        CR_ACCESS_DENIED => Err(Error::msg("Disabling a device needs administrator rights.")),
        r => Err(Error::msg(format!("CM_Disable_DevNode failed (CONFIGRET {r})."))),
    }
}

pub fn probe(args: &[String]) -> Result<Value> {
    match args.first().map(String::as_str) {
        None | Some("list") => Ok(Value::Array(list()?.iter().map(Device::to_json).collect())),
        Some("disconnect") => {
            let id = arg(args, "--id").ok_or_else(|| Error::msg("disconnect needs --id <instance id>."))?;
            if !is_bluetooth_device(id) {
                return Err(Error::msg("Only BTHENUM / BTHLE devices can be disconnected."));
            }
            require_yes(args, "Disconnecting (disabling) a Bluetooth device")?;
            disconnect(id)?;
            Ok(json!({ "ok": true }))
        }
        Some(other) => Err(Error::msg(format!("No bluetooth action {other}. Try list, disconnect."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_the_ps1() {
        assert!(is_bluetooth_device(r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_LOCALMFG&0002\7&1"));
        assert!(is_bluetooth_device(r"BTHLE\DEV_AABBCCDDEEFF\8&1"));
        assert!(is_bluetooth_device(r"bthledevice\{00001812-0000-1000-8000-00805f9b34fb}_dev_vid&01"));
        assert!(!is_bluetooth_device(r"USB\VID_0BDA&PID_8771\00E04C239987"));
        assert!(!is_bluetooth_device(r"BTH\MS_BTHPAN\7&2"));
    }

    #[test]
    fn status_ok_matches_get_pnpdevice() {
        // Raw bits seen on the dev PC, and what Get-PnpDevice called them.
        assert!(is_status_ok(CM_DEVNODE_STATUS_FLAGS(0x0180_200a))); // OK, started
        assert!(is_status_ok(CM_DEVNODE_STATUS_FLAGS(0x0180_2000))); // OK, not started
        assert!(!is_status_ok(CM_DEVNODE_STATUS_FLAGS(0x0180_300b))); // Degraded
        assert!(!is_status_ok(CM_DEVNODE_STATUS_FLAGS(0x0180_2400))); // Error, problem 28
    }

    #[test]
    fn disconnect_checks_its_arguments() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(probe(&a(&["disconnect", "--id", r"BTHLE\DEV_X"])).unwrap_err().to_string().contains("--yes"));
        assert!(probe(&a(&["disconnect", "--id", r"USB\X", "--yes"])).unwrap_err().to_string().contains("BTHENUM"));
        assert!(probe(&a(&["disconnect", "--yes"])).unwrap_err().to_string().contains("--id"));
    }
}
