//! File and folder pickers (Electron's `dialog.showOpenDialog` /
//! `showSaveDialog`). Modal to `owner`: each call blocks the calling (UI)
//! thread until the dialog closes, running its own message loop meanwhile.
//! Nothing in main.js multi-selects, so these return one path or None
//! (cancelled).
use super::{arg, wide, Com, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;

/// (name, extensions without the dot); "*" is all files.
pub type Filter<'a> = (&'a str, &'a [&'a str]);

pub fn open_file(owner: Option<HWND>, filters: &[Filter], folder: Option<&Path>) -> Result<Option<PathBuf>> {
    let _com = Com::init();
    let d: IFileOpenDialog = unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.ctx("Creating the open dialog")?;
    set_filters(&d, filters)?;
    show(&d, owner, FOS_FILEMUSTEXIST, folder)
}

pub fn open_folder(owner: Option<HWND>, folder: Option<&Path>) -> Result<Option<PathBuf>> {
    let _com = Com::init();
    let d: IFileOpenDialog = unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.ctx("Creating the folder dialog")?;
    show(&d, owner, FOS_PICKFOLDERS, folder)
}

/// `file_name` pre-fills the name box; a name typed without an extension
/// gets the first filter's (as Electron does).
pub fn save_file(owner: Option<HWND>, filters: &[Filter], file_name: Option<&str>, folder: Option<&Path>) -> Result<Option<PathBuf>> {
    let _com = Com::init();
    let d: IFileSaveDialog = unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }.ctx("Creating the save dialog")?;
    set_filters(&d, filters)?;
    if let Some(ext) = filters.first().and_then(|f| f.1.first()).filter(|e| **e != "*") {
        unsafe { d.SetDefaultExtension(PCWSTR(wide(ext).as_ptr())) }.ctx("SetDefaultExtension")?;
    }
    if let Some(name) = file_name {
        unsafe { d.SetFileName(PCWSTR(wide(name).as_ptr())) }.ctx("SetFileName")?;
    }
    show(&d, owner, FOS_OVERWRITEPROMPT, folder)
}

fn set_filters(d: &IFileDialog, filters: &[Filter]) -> Result<()> {
    if filters.is_empty() {
        return Ok(());
    }
    let strings: Vec<(Vec<u16>, Vec<u16>)> = filters
        .iter()
        .map(|(name, exts)| (wide(name), wide(exts.iter().map(|e| if *e == "*" { "*.*".into() } else { format!("*.{e}") }).collect::<Vec<_>>().join(";"))))
        .collect();
    // `strings` outlives the call, which copies the specs.
    let specs: Vec<COMDLG_FILTERSPEC> = strings.iter().map(|(n, s)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(s.as_ptr()) }).collect();
    unsafe { d.SetFileTypes(&specs) }.ctx("SetFileTypes")
}

fn show(d: &IFileDialog, owner: Option<HWND>, flags: FILEOPENDIALOGOPTIONS, folder: Option<&Path>) -> Result<Option<PathBuf>> {
    unsafe {
        let opts = d.GetOptions().ctx("GetOptions")?;
        d.SetOptions(opts | flags | FOS_FORCEFILESYSTEM).ctx("SetOptions")?;
        if let Some(f) = folder {
            // A folder that doesn't exist is ignored, as in Electron.
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide(f).as_ptr()), None) {
                let _ = d.SetFolder(&item);
            }
        }
        if let Err(e) = d.Show(owner) {
            return if e.code() == ERROR_CANCELLED.to_hresult() { Ok(None) } else { Err(e).ctx("Showing the dialog") };
        }
        let path = d.GetResult().ctx("GetResult")?.GetDisplayName(SIGDN_FILESYSPATH).ctx("GetDisplayName")?;
        let out = PathBuf::from(path.to_string().unwrap_or_default());
        CoTaskMemFree(Some(path.0 as _));
        Ok(Some(out))
    }
}

/// For the probe: after `secs`, posts WM_CLOSE to this process's dialogs, so
/// a probe run never waits for (or makes) a real choice.
fn close_dialogs_after(secs: f64) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
        unsafe extern "system" fn each(hwnd: HWND, _: LPARAM) -> windows::core::BOOL {
            let mut pid = 0;
            let mut class = [0u16; 16];
            unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                GetClassNameW(hwnd, &mut class);
                if pid == std::process::id() && super::from_wide(&class) == "#32770" {
                    eprintln!("closing the dialog");
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            true.into()
        }
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(0));
        }
    });
}

pub fn probe(args: &[String]) -> Result<Value> {
    let secs: f64 = arg(args, "--close-after").and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let folder = arg(args, "--folder").map(Path::new);
    let action = args.first().map(String::as_str).unwrap_or("");
    if !["open-file", "open-folder", "save"].contains(&action) {
        return Err(Error::msg("Usage: dialogs open-file|open-folder|save [--folder <dir>] [--close-after <seconds>, default 3]"));
    }
    close_dialogs_after(secs);
    let picked = match action {
        "open-file" => open_file(
            None,
            &[("Programs & Shortcuts", &["exe", "lnk", "bat", "cmd"]), ("Scripts", &["py", "js", "mjs", "ps1"]), ("All Files", &["*"])],
            folder,
        )?,
        "open-folder" => open_folder(None, folder)?,
        _ => save_file(None, &[("Launchpad Backup", &["json"])], Some("launchpad-backup-probe.json"), folder)?,
    };
    Ok(json!({ "action": action, "picked": picked, "cancelled": picked.is_none() }))
}
