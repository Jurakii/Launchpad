//! Wi-Fi through the Native Wifi API (wlanapi.dll). Replaces lib\wifi.ps1,
//! which parsed `netsh wlan` text.
//!
//! SSIDs are up to 32 raw bytes with no declared encoding. They are decoded
//! as UTF-8, and bytes that aren't valid UTF-8 become U+FFFD (lossy).
use super::{arg, require_yes, wide, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use windows::core::{w, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// One visible network, de-duplicated by SSID. The first four fields are the
/// .ps1's; `signal`, `secured` and `profile` are new.
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// netsh's wording, e.g. "WPA2-Personal", "Open".
    pub auth: &'static str,
    pub connected: bool,
    /// A saved profile exists for this network.
    pub saved: bool,
    /// 0–100.
    pub signal: u32,
    pub secured: bool,
    /// The saved profile's name, what `connect` takes.
    pub profile: Option<String>,
}

impl Network {
    pub fn to_json(&self) -> Value {
        json!({
            "ssid": self.ssid, "auth": self.auth, "connected": self.connected, "saved": self.saved,
            "signal": self.signal, "secured": self.secured, "profile": self.profile,
        })
    }
}

const ERROR_SERVICE_NOT_ACTIVE: u32 = 1062;

fn check(code: u32, what: &str) -> Result<()> {
    HRESULT::from_win32(code).ok().ctx(what)
}

/// A wlanapi client handle, closed on drop.
struct Client(HANDLE);

impl Client {
    fn open_code() -> std::result::Result<Client, u32> {
        let (mut version, mut h) = (0, HANDLE::default());
        match unsafe { WlanOpenHandle(WLAN_API_VERSION_2_0, None, &mut version, &mut h) } {
            0 => Ok(Client(h)),
            e => Err(e),
        }
    }

    fn open() -> Result<Client> {
        Self::open_code().map_err(|e| check(e, "WlanOpenHandle").unwrap_err())
    }

    fn interfaces(&self) -> Result<Vec<GUID>> {
        let mut list = std::ptr::null_mut();
        check(unsafe { WlanEnumInterfaces(self.0, None, &mut list) }, "WlanEnumInterfaces")?;
        // Safety: on success wlanapi returned a list with dwNumberOfItems entries; freed right after copying.
        let guids = unsafe {
            let l = &*list;
            let items = std::slice::from_raw_parts(l.InterfaceInfo.as_ptr(), l.dwNumberOfItems as usize);
            let g = items.iter().map(|i| i.InterfaceGuid).collect();
            WlanFreeMemory(list as *const c_void);
            g
        };
        Ok(guids)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

fn auth_name(a: DOT11_AUTH_ALGORITHM) -> &'static str {
    // Strings as `netsh wlan show networks` prints them.
    match a.0 {
        1 => "Open",
        2 => "Shared",
        3 => "WPA-Enterprise",
        4 => "WPA-Personal",
        5 => "WPA-None",
        6 => "WPA2-Enterprise",
        7 => "WPA2-Personal",
        8 => "WPA3-Enterprise 192 Bits",
        9 => "WPA3-Personal",
        10 => "OWE",
        11 => "WPA3-Enterprise",
        _ => "Unknown",
    }
}

/// wlanapi lists a network once per profile plus once without; the .ps1 (via
/// netsh) showed each SSID once. Hidden networks (empty SSID) are dropped.
fn dedupe(raw: Vec<Network>) -> Vec<Network> {
    let mut out: Vec<Network> = Vec::new();
    for n in raw.into_iter().filter(|n| !n.ssid.is_empty()) {
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) => {
                o.connected |= n.connected;
                o.saved |= n.saved;
                o.signal = o.signal.max(n.signal);
                if o.profile.is_none() {
                    o.profile = n.profile;
                }
            }
            None => out.push(n),
        }
    }
    out
}

/// Visible networks on every Wi-Fi adapter, from Windows' last scan (like netsh).
/// No Wi-Fi service (a PC without Wi-Fi) gives an empty list, as the .ps1 did.
pub fn list() -> Result<Vec<Network>> {
    let client = match Client::open_code() {
        Ok(c) => c,
        Err(ERROR_SERVICE_NOT_ACTIVE) => return Ok(Vec::new()),
        Err(e) => return check(e, "WlanOpenHandle").map(|_| Vec::new()),
    };
    let mut raw = Vec::new();
    for guid in client.interfaces()? {
        let mut list = std::ptr::null_mut();
        check(unsafe { WlanGetAvailableNetworkList(client.0, &guid, 0, None, &mut list) }, "WlanGetAvailableNetworkList")?;
        // Safety: as in `interfaces`.
        unsafe {
            let l = &*list;
            for n in std::slice::from_raw_parts(l.Network.as_ptr(), l.dwNumberOfItems as usize) {
                let len = (n.dot11Ssid.uSSIDLength as usize).min(32);
                let has_profile = n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0;
                raw.push(Network {
                    ssid: String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).into_owned(),
                    auth: auth_name(n.dot11DefaultAuthAlgorithm),
                    connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                    saved: has_profile,
                    signal: n.wlanSignalQuality,
                    secured: n.bSecurityEnabled.as_bool(),
                    profile: has_profile.then(|| super::from_wide(&n.strProfileName)),
                });
            }
            WlanFreeMemory(list as *const c_void);
        }
    }
    Ok(dedupe(raw))
}

fn first_interface(client: &Client) -> Result<GUID> {
    // shortcut: the first adapter only, like netsh without interface=; pick by profile if PCs with two Wi-Fi adapters matter.
    client.interfaces()?.into_iter().next().ok_or_else(|| Error::msg("No Wi-Fi adapter."))
}

/// Connect with a saved profile. Returns once Windows accepted the request;
/// the outcome arrives as `Event::Connected` (or nothing, on failure).
pub fn connect(profile: &str) -> Result<()> {
    if profile.is_empty() || profile.chars().count() > 255 {
        return Err(Error::msg("A profile name is 1 to 255 characters."));
    }
    let client = Client::open()?;
    let guid = first_interface(&client)?;
    let name = wide(profile);
    let params = WLAN_CONNECTION_PARAMETERS {
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: PCWSTR(name.as_ptr()),
        dot11BssType: dot11_BSS_type_any,
        ..Default::default()
    };
    check(unsafe { WlanConnect(client.0, &guid, &params, None) }, "WlanConnect")
}

/// Disconnect every Wi-Fi adapter.
pub fn disconnect() -> Result<()> {
    let client = Client::open()?;
    for guid in client.interfaces()? {
        check(unsafe { WlanDisconnect(client.0, &guid, None) }, "WlanDisconnect")?;
    }
    Ok(())
}

/// Ask every adapter for a fresh scan; `Event::ScanComplete` follows (2–4 s).
pub fn scan() -> Result<()> {
    let client = Client::open()?;
    for guid in client.interfaces()? {
        check(unsafe { WlanScan(client.0, &guid, None, None, None) }, "WlanScan")?;
    }
    Ok(())
}

/// Windows' network settings page, as main.js opened it.
pub fn open_settings() -> Result<()> {
    let r = unsafe { ShellExecuteW(None, w!("open"), w!("ms-settings:network-status"), None, None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(Error::msg(format!("Couldn't open network settings (ShellExecute {}).", r.0 as isize)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Connected,
    Disconnected,
    /// Also sent when a scan fails, so a "scanning" state always ends.
    ScanComplete,
}

type Callback = Box<dyn Fn(Event) + Send + Sync>;

/// Wi-Fi change notifications; unregistered on drop. Don't drop it from
/// inside its own callback (wlanapi would deadlock waiting for it).
pub struct Watch {
    client: Client,
    ctx: *mut Callback,
}

// Safety: the handle and the boxed Send + Sync callback may move threads.
unsafe impl Send for Watch {}

/// `f` runs on a wlanapi worker thread, so it should only post a message.
pub fn watch(f: impl Fn(Event) + Send + Sync + 'static) -> Result<Watch> {
    let client = Client::open()?;
    let ctx = Box::into_raw(Box::new(Box::new(f) as Callback));
    let r = unsafe {
        WlanRegisterNotification(client.0, WLAN_NOTIFICATION_SOURCE_ACM, true, Some(on_notify), Some(ctx as *const c_void), None, None)
    };
    if let Err(e) = check(r, "WlanRegisterNotification") {
        drop(unsafe { Box::from_raw(ctx) });
        return Err(e);
    }
    Ok(Watch { client, ctx })
}

impl Drop for Watch {
    fn drop(&mut self) {
        // Unregistering blocks until a callback in progress returns, so ctx is free to go after it.
        unsafe {
            WlanRegisterNotification(self.client.0, WLAN_NOTIFICATION_SOURCE_NONE, true, None, None, None, None);
            drop(Box::from_raw(self.ctx));
        }
    }
}

#[allow(non_upper_case_globals)] // wlanapi's constant names
unsafe extern "system" fn on_notify(data: *mut L2_NOTIFICATION_DATA, ctx: *mut c_void) {
    // Safety: wlanapi passes valid data and the ctx registered in `watch`, alive until unregistered.
    let d = unsafe { &*data };
    if d.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM {
        return;
    }
    let event = match WLAN_NOTIFICATION_ACM(d.NotificationCode as i32) {
        wlan_notification_acm_connection_complete => {
            let ok = !d.pData.is_null()
                && d.dwDataSize as usize >= std::mem::size_of::<WLAN_CONNECTION_NOTIFICATION_DATA>()
                && unsafe { (*(d.pData as *const WLAN_CONNECTION_NOTIFICATION_DATA)).wlanReasonCode } == 0;
            if !ok {
                return;
            }
            Event::Connected
        }
        wlan_notification_acm_disconnected => Event::Disconnected,
        wlan_notification_acm_scan_complete | wlan_notification_acm_scan_fail => Event::ScanComplete,
        _ => return,
    };
    unsafe { (*(ctx as *const Callback))(event) };
}

pub fn probe(args: &[String]) -> Result<Value> {
    match args.first().map(String::as_str) {
        None | Some("list") => Ok(Value::Array(list()?.iter().map(Network::to_json).collect())),
        Some("connect") => {
            let profile = arg(args, "--profile").ok_or_else(|| Error::msg("connect needs --profile <name>."))?;
            require_yes(args, "Connecting to Wi-Fi")?;
            connect(profile)?;
            Ok(json!({ "ok": true }))
        }
        Some("disconnect") => {
            require_yes(args, "Disconnecting Wi-Fi")?;
            disconnect()?;
            Ok(json!({ "ok": true }))
        }
        Some("settings") => {
            open_settings()?;
            Ok(json!({ "ok": true }))
        }
        Some("watch") => {
            let secs: u64 = match args.get(1) {
                Some(s) => s.parse().map_err(|_| Error::msg("watch [seconds]: seconds is a number."))?,
                None => 10,
            };
            let start = std::time::Instant::now();
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = events.clone();
            let _w = watch(move |e| {
                eprintln!("{:?}", e);
                sink.lock().unwrap().push(json!({ "event": format!("{e:?}"), "ms": start.elapsed().as_millis() as u64 }));
            })?;
            scan()?; // so at least a ScanComplete shows up
            std::thread::sleep(std::time::Duration::from_secs(secs));
            drop(_w);
            let events = events.lock().unwrap().clone();
            Ok(json!({ "seconds": secs, "events": events }))
        }
        Some(other) => Err(Error::msg(format!("No wifi action {other}. Try list, connect, disconnect, settings, watch."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, connected: bool, profile: Option<&str>, signal: u32) -> Network {
        Network {
            ssid: ssid.into(),
            auth: "WPA2-Personal",
            connected,
            saved: profile.is_some(),
            signal,
            secured: true,
            profile: profile.map(String::from),
        }
    }

    #[test]
    fn dedupe_merges_by_ssid_and_drops_hidden() {
        let out = dedupe(vec![
            net("Home", false, None, 40),
            net("", false, None, 90),
            net("Café", false, None, 10),
            net("Home", true, Some("Home 2"), 70),
            net("home", false, None, 5),
        ]);
        assert_eq!(out.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), ["Home", "Café", "home"]);
        assert_eq!(out[0], Network { connected: true, saved: true, signal: 70, profile: Some("Home 2".into()), ..net("Home", false, None, 0) });
    }

    #[test]
    fn auth_names_match_netsh() {
        assert_eq!(auth_name(DOT11_AUTH_ALGO_RSNA_PSK), "WPA2-Personal");
        assert_eq!(auth_name(DOT11_AUTH_ALGO_WPA3_SAE), "WPA3-Personal");
        assert_eq!(auth_name(DOT11_AUTH_ALGO_80211_OPEN), "Open");
    }

    #[test]
    fn actions_refuse_without_yes() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(probe(&a(&["connect", "--profile", "X"])).unwrap_err().to_string().contains("--yes"));
        assert!(probe(&a(&["connect", "--yes"])).unwrap_err().to_string().contains("--profile"));
        assert!(probe(&a(&["disconnect"])).unwrap_err().to_string().contains("--yes"));
        assert!(connect("").is_err());
    }
}
