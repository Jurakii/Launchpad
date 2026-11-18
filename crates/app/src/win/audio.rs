//! Audio: master volume and mute of the default output device, the output
//! device list, switching the default device, per-app sessions, and change
//! events. Replaces lib\volume.ps1 and lib\audio-devices.ps1.
//!
//! Every function needs COM initialised on the calling thread (`Com::init()`;
//! the MMDevice objects are free-threaded, so either apartment works).
//! "Default device" means the default render endpoint for the multimedia
//! role, as the .ps1s read it.

use super::{arg, from_wide, require_yes, Context, Com};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, Weak};
use windows::core::{implement, Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, PROPERTYKEY};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl};
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, AudioSessionStateExpired, EDataFlow, ERole, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, ISimpleAudioVolume, MMDeviceEnumerator,
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};

#[derive(Debug, Clone, PartialEq)]
pub struct Master {
    pub volume: u32,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub pid: u32,
    /// The process's exe name without ".exe", as .NET's Process.ProcessName gave it.
    pub name: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    /// Master volume or mute of the default device changed (also sent once
    /// after the default device changes, with the new device's values).
    Volume(Master),
    /// The default output device changed; `None` when there is none.
    DefaultDevice(Option<String>),
    /// An endpoint was added, removed, or changed state (plugged, unplugged,
    /// disabled). Not filtered by direction: re-read `devices()`.
    DevicesChanged(String),
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ctx("Core Audio")
}

fn default_device(e: &IMMDeviceEnumerator) -> Result<IMMDevice> {
    unsafe { e.GetDefaultAudioEndpoint(eRender, eMultimedia) }.ctx("Default output device")
}

fn endpoint_volume() -> Result<IAudioEndpointVolume> {
    unsafe { default_device(&enumerator()?)?.Activate(CLSCTX_ALL, None) }.ctx("Endpoint volume")
}

/// A string Core Audio allocated for us; freed here.
fn take(p: PWSTR) -> String {
    // Safety: p is a NUL-terminated CoTaskMemAlloc'd string we now own.
    unsafe {
        let s = from_wide(p.as_wide());
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// Scalar 0.0–1.0 to percent, rounding half to even like .NET's Math.Round.
fn pct(level: f32) -> u32 {
    (f64::from(level) * 100.0).round_ties_even().clamp(0.0, 100.0) as u32
}

fn scalar(pct: u32) -> f32 {
    pct.min(100) as f32 / 100.0
}

fn read_master(v: &IAudioEndpointVolume) -> Result<Master> {
    unsafe {
        Ok(Master {
            volume: pct(v.GetMasterVolumeLevelScalar().ctx("Read volume")?),
            muted: v.GetMute().ctx("Read mute")?.as_bool(),
        })
    }
}

pub fn master() -> Result<Master> {
    read_master(&endpoint_volume()?)
}

/// 0–100, clamped.
pub fn set_volume(pct: u32) -> Result<()> {
    unsafe { endpoint_volume()?.SetMasterVolumeLevelScalar(scalar(pct), std::ptr::null()) }.ctx("Set volume")
}

pub fn set_muted(muted: bool) -> Result<()> {
    unsafe { endpoint_volume()?.SetMute(muted, std::ptr::null()) }.ctx("Set mute")
}

/// Active output devices.
pub fn devices() -> Result<Vec<Device>> {
    let e = enumerator()?;
    let default_id = default_device(&e).ok().and_then(|d| unsafe { d.GetId() }.ok()).map(take);
    let list = unsafe { e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }.ctx("List output devices")?;
    let mut out = Vec::new();
    for i in 0..unsafe { list.GetCount() }.ctx("List output devices")? {
        let dev = unsafe { list.Item(i) }.ctx("Output device")?;
        let id = take(unsafe { dev.GetId() }.ctx("Device id")?);
        let name = unsafe { dev.OpenPropertyStore(STGM_READ).and_then(|s| s.GetValue(&PKEY_Device_FriendlyName)) }
            .map(|v| v.to_string())
            .ok()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Unknown device".into());
        out.push(Device { is_default: default_id.as_deref() == Some(id.as_str()), id, name });
    }
    Ok(out)
}

// IPolicyConfig is undocumented: these are the exact GUIDs lib\audio-devices.ps1
// used (stable since Windows 7). Only slot order matters for the methods we
// don't call, so they are declared without parameters.
#[allow(non_snake_case)] // the COM method names (#[interface] takes no attributes itself)
mod policy {
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;

    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self) -> HRESULT;
        fn GetDeviceFormat(&self) -> HRESULT;
        fn ResetDeviceFormat(&self) -> HRESULT;
        fn SetDeviceFormat(&self) -> HRESULT;
        fn GetProcessingPeriod(&self) -> HRESULT;
        fn SetProcessingPeriod(&self) -> HRESULT;
        fn GetShareMode(&self) -> HRESULT;
        fn SetShareMode(&self) -> HRESULT;
        fn GetPropertyValue(&self) -> HRESULT;
        fn SetPropertyValue(&self) -> HRESULT;
        fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self) -> HRESULT;
    }

    /// The generated methods are private to this module, so the call lives here.
    pub fn set_default(id: PCWSTR, role: ERole) -> windows::core::Result<()> {
        const CLIENT: windows::core::GUID = windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
        unsafe {
            let p: IPolicyConfig = windows::Win32::System::Com::CoCreateInstance(&CLIENT, None, windows::Win32::System::Com::CLSCTX_ALL)?;
            p.SetDefaultEndpoint(id, role).ok()
        }
    }
}

/// Makes `id` (from `devices()`) the default output for all three roles.
pub fn set_default_device(id: &str) -> Result<()> {
    if !devices()?.iter().any(|d| d.id == id) {
        return Err(Error::msg(format!("No active output device with id {id}.")));
    }
    let wid = super::wide(id);
    for role in 0..3 {
        policy::set_default(PCWSTR(wid.as_ptr()), ERole(role)).ctx("Set default device")?;
    }
    Ok(())
}

/// Calls `f` with every session of the default device, expired ones included.
fn each_session(mut f: impl FnMut(IAudioSessionControl2, ISimpleAudioVolume) -> Result<()>) -> Result<()> {
    let mgr: IAudioSessionManager2 = unsafe { default_device(&enumerator()?)?.Activate(CLSCTX_ALL, None) }.ctx("Session manager")?;
    let list = unsafe { mgr.GetSessionEnumerator() }.ctx("List sessions")?;
    for i in 0..unsafe { list.GetCount() }.ctx("List sessions")? {
        let s = unsafe { list.GetSession(i) }.ctx("Session")?;
        f(s.cast().ctx("Session")?, s.cast().ctx("Session volume")?)?;
    }
    Ok(())
}

/// Exe names by pid, from one process snapshot.
fn process_names() -> std::collections::HashMap<u32, String> {
    let mut names = std::collections::HashMap::new();
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return names };
    let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut ok = unsafe { Process32FirstW(snap, &mut e) }.is_ok();
    while ok {
        names.insert(e.th32ProcessID, from_wide(&e.szExeFile));
        ok = unsafe { Process32NextW(snap, &mut e) }.is_ok();
    }
    unsafe { _ = CloseHandle(snap) };
    names
}

/// What .NET's Process.ProcessName shows: the exe name minus ".exe".
fn display_name(pid: u32, exe: Option<&str>) -> String {
    match exe {
        Some(n) if !n.is_empty() => match n.len().checked_sub(4) {
            Some(i) if n.is_char_boundary(i) && n[i..].eq_ignore_ascii_case(".exe") => n[..i].to_string(),
            _ => n.to_string(),
        },
        _ => format!("pid {pid}"),
    }
}

/// Per-app sessions on the default device. Skips expired sessions and the
/// system-sounds session (pid 0), as the .ps1 did.
pub fn sessions() -> Result<Vec<Session>> {
    let names = process_names();
    let mut out = Vec::new();
    each_session(|ctl, vol| unsafe {
        let pid = ctl.GetProcessId().unwrap_or(0);
        if ctl.GetState().is_ok_and(|s| s == AudioSessionStateExpired) || pid == 0 {
            return Ok(());
        }
        out.push(Session {
            pid,
            name: display_name(pid, names.get(&pid).map(String::as_str)),
            volume: vol.GetMasterVolume().map(pct).unwrap_or(0),
            muted: vol.GetMute().is_ok_and(BOOL::as_bool),
        });
        Ok(())
    })?;
    Ok(out)
}

/// Applies to every session of `pid`; errors if it has none.
fn for_pid(pid: u32, f: impl Fn(&ISimpleAudioVolume) -> windows::core::Result<()>) -> Result<()> {
    let mut found = false;
    each_session(|ctl, vol| {
        if unsafe { ctl.GetProcessId() }.is_ok_and(|p| p == pid) {
            found = true;
            f(&vol).ctx("Set session")?;
        }
        Ok(())
    })?;
    if found { Ok(()) } else { Err(Error::msg(format!("No audio session for pid {pid}."))) }
}

pub fn set_session_volume(pid: u32, pct: u32) -> Result<()> {
    for_pid(pid, |v| unsafe { v.SetMasterVolume(scalar(pct), std::ptr::null()) })
}

pub fn set_session_muted(pid: u32, muted: bool) -> Result<()> {
    for_pid(pid, |v| unsafe { v.SetMute(muted, std::ptr::null()) })
}

// ---- Change events -------------------------------------------------------

struct Shared {
    emit: Mutex<Box<dyn Fn(AudioEvent) + Send>>,
    vol: Mutex<VolState>,
}

struct VolState {
    closed: bool,
    callback: IAudioEndpointVolumeCallback,
    current: Option<IAudioEndpointVolume>,
}
// Safety: the MMDevice API objects are free-threaded, and our callback object
// only holds a Weak<Shared>.
unsafe impl Send for VolState {}

impl Shared {
    fn emit(&self, e: AudioEvent) {
        if let Ok(f) = self.emit.lock() {
            f(e);
        }
    }

    /// Moves the volume callback onto the current default device.
    fn follow_default(&self) -> Result<()> {
        let mut st = self.vol.lock().map_err(|_| Error::msg("Audio watch poisoned"))?;
        if st.closed {
            return Ok(());
        }
        if let Some(old) = st.current.take() {
            unsafe { _ = old.UnregisterControlChangeNotify(&st.callback) };
        }
        let Ok(v) = endpoint_volume() else { return Ok(()) }; // no output device right now
        unsafe { v.RegisterControlChangeNotify(&st.callback) }.ctx("Watch volume")?;
        let m = read_master(&v);
        st.current = Some(v);
        drop(st);
        if let Ok(m) = m {
            self.emit(AudioEvent::Volume(m));
        }
        Ok(())
    }
}

// The volume callback holds a Weak so Shared -> callback -> Shared isn't a cycle.
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback(Weak<Shared>);

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, d: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        // Safety: Core Audio passes a valid pointer (or null) for the duration of the call.
        if let (Some(d), Some(shared)) = (unsafe { d.as_ref() }, self.0.upgrade()) {
            shared.emit(AudioEvent::Volume(Master { volume: pct(d.fMasterVolume), muted: d.bMuted.as_bool() }));
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceClient(Arc<Shared>);

fn id_of(p: &PCWSTR) -> Option<String> {
    // Safety: a null or NUL-terminated string valid for the callback.
    (!p.is_null()).then(|| from_wide(unsafe { p.as_wide() }))
}

impl IMMNotificationClient_Impl for DeviceClient_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        self.OnDeviceAdded(id)
    }
    fn OnDeviceAdded(&self, id: &PCWSTR) -> windows::core::Result<()> {
        self.0.emit(AudioEvent::DevicesChanged(id_of(id).unwrap_or_default()));
        Ok(())
    }
    fn OnDeviceRemoved(&self, id: &PCWSTR) -> windows::core::Result<()> {
        self.OnDeviceAdded(id)
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, id: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eMultimedia {
            self.0.emit(AudioEvent::DefaultDevice(id_of(id)));
            // Core Audio forbids releasing its objects inside this callback, so
            // the volume callback moves to the new device on a short-lived thread.
            let shared = self.0.clone();
            std::thread::spawn(move || {
                let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                _ = shared.follow_default();
                if inited {
                    unsafe { CoUninitialize() };
                }
            });
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Unregisters everything on drop.
pub struct Watch {
    shared: Arc<Shared>,
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

/// Calls `f` on every audio change until the returned `Watch` is dropped.
/// `f` runs on Core Audio's worker threads (never the caller's), one call at
/// a time; keep it quick and non-blocking (post to the UI thread). The volume
/// events follow the default device when it changes.
pub fn watch(f: impl Fn(AudioEvent) + Send + 'static) -> Result<Watch> {
    let shared = Arc::new_cyclic(|weak| Shared {
        emit: Mutex::new(Box::new(f)),
        vol: Mutex::new(VolState { closed: false, callback: VolumeCallback(weak.clone()).into(), current: None }),
    });
    let enumerator = enumerator()?;
    let client: IMMNotificationClient = DeviceClient(shared.clone()).into();
    unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }.ctx("Watch devices")?;
    let w = Watch { shared, enumerator, client };
    w.shared.follow_default()?;
    Ok(w)
}

impl Drop for Watch {
    fn drop(&mut self) {
        unsafe { _ = self.enumerator.UnregisterEndpointNotificationCallback(&self.client) };
        if let Ok(mut st) = self.shared.vol.lock() {
            st.closed = true;
            if let Some(v) = st.current.take() {
                unsafe { _ = v.UnregisterControlChangeNotify(&st.callback) };
            }
        }
    }
}

// ---- Probe ---------------------------------------------------------------

fn master_json(m: &Master) -> Value {
    json!({ "volume": m.volume, "muted": m.muted })
}

fn event_json(e: &AudioEvent) -> Value {
    match e {
        AudioEvent::Volume(m) => json!({ "event": "volume", "volume": m.volume, "muted": m.muted }),
        AudioEvent::DefaultDevice(id) => json!({ "event": "defaultDevice", "id": id }),
        AudioEvent::DevicesChanged(id) => json!({ "event": "devicesChanged", "id": id }),
    }
}

/// The first argument that isn't a flag or a flag's value (`--yes` takes none).
fn positional(args: &[String]) -> Option<&str> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--yes" {
            continue;
        }
        if a.starts_with("--") {
            it.next();
            continue;
        }
        return Some(a);
    }
    None
}

fn parse_pct(s: Option<&str>) -> Result<u32> {
    s.and_then(|s| s.parse::<u32>().ok())
        .filter(|&p| p <= 100)
        .ok_or_else(|| Error::msg("Give a volume from 0 to 100."))
}

fn parse_bool(s: Option<&str>) -> Result<bool> {
    match s {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(Error::msg("Give true or false.")),
    }
}

fn parse_pid(args: &[String]) -> Result<u32> {
    arg(args, "--pid").and_then(|s| s.parse().ok()).ok_or_else(|| Error::msg("Give --pid N."))
}

pub fn probe(args: &[String]) -> Result<Value> {
    let _com = Com::init();
    let action = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    let ok = json!({ "ok": true });
    match action {
        None => {
            let m = master()?;
            let devices: Vec<Value> =
                devices()?.iter().map(|d| json!({ "id": d.id, "name": d.name, "isDefault": d.is_default })).collect();
            let sessions: Vec<Value> = sessions()?
                .iter()
                .map(|s| json!({ "pid": s.pid, "name": s.name, "volume": s.volume, "muted": s.muted }))
                .collect();
            Ok(json!({ "volume": m.volume, "muted": m.muted, "devices": devices, "sessions": sessions }))
        }
        Some("set-volume") => {
            set_volume(parse_pct(positional(rest))?)?;
            Ok(master_json(&master()?))
        }
        Some("mute") => {
            set_muted(parse_bool(positional(rest))?)?;
            Ok(master_json(&master()?))
        }
        Some("session-volume") => {
            let pid = parse_pid(rest)?;
            set_session_volume(pid, parse_pct(positional(rest))?)?;
            Ok(ok)
        }
        Some("session-mute") => {
            let pid = parse_pid(rest)?;
            set_session_muted(pid, parse_bool(positional(rest))?)?;
            Ok(ok)
        }
        Some("set-device") => {
            let id = arg(rest, "--id").ok_or_else(|| Error::msg("Give --id <device id> (see the device list)."))?;
            require_yes(rest, "set-device")?;
            set_default_device(id)?;
            Ok(ok)
        }
        Some("watch") => {
            let secs: u64 = match positional(rest) {
                None => 10,
                Some(s) => s.parse().map_err(|_| Error::msg("Give the seconds to watch."))?,
            };
            let w = watch(|e| println!("{}", event_json(&e)))?;
            std::thread::sleep(std::time::Duration::from_secs(secs)); // the watch's lifetime, not a poll
            drop(w);
            Ok(json!({ "watched": secs }))
        }
        Some(a) => Err(Error::msg(format!(
            "Unknown audio action {a}. Try set-volume, mute, session-volume, session-mute, set-device, watch."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn display_name_drops_exe_like_dotnet() {
        assert_eq!(display_name(4, Some("Spotify.exe")), "Spotify");
        assert_eq!(display_name(4, Some("CHROME.EXE")), "CHROME");
        assert_eq!(display_name(4, Some("Café 日本.exe")), "Café 日本");
        assert_eq!(display_name(4, Some("tool.bin")), "tool.bin");
        assert_eq!(display_name(4, Some("日.exe")), "日");
        assert_eq!(display_name(4, Some("ex")), "ex");
        assert_eq!(display_name(42, None), "pid 42");
        assert_eq!(display_name(42, Some("")), "pid 42");
    }

    #[test]
    fn percent_rounds_like_math_round() {
        assert_eq!(pct(0.125), 12); // half to even
        assert_eq!(pct(0.135), 14);
        assert_eq!(pct(1.0), 100);
        assert_eq!(pct(0.0), 0);
        assert_eq!(scalar(150), 1.0);
    }

    #[test]
    fn probe_arguments() {
        assert_eq!(positional(&v(&["--pid", "12", "50"])), Some("50"));
        assert_eq!(positional(&v(&["50", "--pid", "12"])), Some("50"));
        assert_eq!(positional(&v(&["--yes", "7"])), Some("7"));
        assert_eq!(positional(&v(&["--pid", "12"])), None);
        assert_eq!(parse_pid(&v(&["--pid", "12", "50"])).unwrap(), 12);
        assert!(parse_pid(&v(&["50"])).is_err());
        assert_eq!(parse_pct(Some("100")).unwrap(), 100);
        assert!(parse_pct(Some("101")).is_err());
        assert!(parse_pct(Some("-1")).is_err());
        assert!(parse_bool(Some("yes")).is_err());
        assert!(parse_bool(Some("false")).is_ok_and(|b| !b));
    }

    #[test]
    fn set_device_needs_id_then_yes() {
        let e = probe(&v(&["set-device"])).unwrap_err().to_string();
        assert!(e.contains("--id"), "{e}");
        let e = probe(&v(&["set-device", "--id", "x"])).unwrap_err().to_string();
        assert!(e.contains("--yes"), "{e}");
        // An id that isn't an active output device is refused before IPolicyConfig.
        let e = probe(&v(&["set-device", "--id", "not-a-device", "--yes"])).unwrap_err().to_string();
        assert!(e.contains("No active output device"), "{e}");
    }
}
