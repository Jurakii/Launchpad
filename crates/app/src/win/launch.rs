//! Launching: grid items as lib/launcher.js did, Electron's `shell.*`
//! (openPath, openExternal, readShortcutLink, showItemInFolder) and the
//! tracked processes Media starts.
//!
//! Processes start with CreateProcessW, not std::process::Command: Command
//! can't give Node's `detached` (DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
//! together with a verbatim `cmd.exe /d /s /c "…"` line and our own handle
//! for the exit waiter, and the quoting is a dozen lines tested below
//! against CommandLineToArgvW. Everything here may block briefly (the shell
//! calls especially): call it off the UI thread.
use super::{arg, from_wide, wide, Com, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Map, Value};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::Arc;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, ERROR_NO_ASSOCIATION};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
    INFINITE, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    ILFree, IShellLinkW, SHOpenFolderAndSelectItems, SHParseDisplayName, ShellExecuteExW, ShellLink, SEE_MASK_FLAG_NO_UI,
    SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SLGP_UNCPRIORITY,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWDEFAULT, SW_SHOWNORMAL};

// ---- Argument handling ----

/// JavaScript's `\s`.
fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// launcher.js's splitArgs: `str.match(/"[^"]*"|'[^']*'|\S+/g)`, then one
/// leading and one trailing quote of either kind stripped from each.
pub fn split_args(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if is_js_space(c[i]) {
            i += 1;
            continue;
        }
        let quoted = matches!(c[i], '"' | '\'') && c[i + 1..].contains(&c[i]);
        let end = if quoted {
            i + 2 + c[i + 1..].iter().position(|&x| x == c[i]).unwrap()
        } else {
            i + c[i..].iter().position(|&x| is_js_space(x)).unwrap_or(c.len() - i)
        };
        let mut m = &c[i..end];
        if let [first, rest @ ..] = m {
            if matches!(first, '"' | '\'') {
                m = rest;
            }
        }
        if let [rest @ .., last] = m {
            if matches!(last, '"' | '\'') {
                m = rest;
            }
        }
        out.push(m.iter().collect());
        i = end;
    }
    out
}

/// One argument, quoted so CommandLineToArgvW (and the MSVC runtime) reads
/// it back unchanged.
fn quote_arg(a: &str, out: &mut String) {
    if !a.is_empty() && !a.contains([' ', '\t', '\n', '\x0b', '"']) {
        out.push_str(a);
        return;
    }
    out.push('"');
    let mut backslashes = 0;
    for ch in a.chars() {
        if ch == '\\' {
            backslashes += 1;
        } else {
            if ch == '"' {
                // Double the backslashes before a quote, then escape the quote.
                out.extend(std::iter::repeat_n('\\', backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(ch);
    }
    // Backslashes before the closing quote are doubled too.
    out.extend(std::iter::repeat_n('\\', backslashes));
    out.push('"');
}

/// `exe` then `args`, as a Windows command line. The program name follows
/// its own rule (quotes only group, no escapes), and can't contain `"`.
pub fn command_line(exe: &str, args: &[String]) -> String {
    let mut line = if exe.is_empty() || exe.contains([' ', '\t']) { format!("\"{exe}\"") } else { exe.to_string() };
    for a in args {
        line.push(' ');
        quote_arg(a, &mut line);
    }
    line
}

/// A word for cmd.exe: quoted when it has spaces or characters cmd treats specially.
fn cmd_word(s: &str) -> String {
    if !s.is_empty() && !s.contains([' ', '\t', '&', '|', '<', '>', '^', '(', ')', ',', ';', '=']) {
        s.to_string()
    } else {
        format!("\"{s}\"")
    }
}

// ---- Processes ----

/// A started process: its id and handle.
pub struct Process {
    pub pid: u32,
    pub handle: OwnedHandle,
}

/// CreateProcessW with `line` verbatim. `detached` is Node's `detached:
/// true` on Windows (no console shared with us, own process group). The
/// window is shown normally and no handles are inherited (stdio 'ignore').
pub fn spawn_line(line: &str, cwd: &str, detached: bool) -> Result<Process> {
    let mut line_w = wide(line);
    let cwd_w = (!cwd.is_empty()).then(|| wide(cwd));
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_SHOWDEFAULT.0 as u16,
        ..Default::default()
    };
    let flags = if detached { DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP } else { PROCESS_CREATION_FLAGS(0) };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(line_w.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            None,
            cwd_w.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
            &si,
            &mut pi,
        )
    }
    .map_err(|e| Error::msg(format!("Couldn't start {line}: {}", e.message())))?;
    // SAFETY: CreateProcessW succeeded, so both handles are ours to close.
    unsafe { drop(OwnedHandle::from_raw_handle(pi.hThread.0)) };
    Ok(Process { pid: pi.dwProcessId, handle: unsafe { OwnedHandle::from_raw_handle(pi.hProcess.0) } })
}

/// Starts `exe` with `args` as separate arguments, no shell.
pub fn spawn(exe: &str, args: &[String], cwd: &str, detached: bool) -> Result<Process> {
    spawn_line(&command_line(exe, args), cwd, detached)
}

/// launcher.js's detachedSpawn. With `shell` (or a .cmd/.bat target, which
/// Node can only run through the shell) it runs `%ComSpec% /d /s /c "<line>"`
/// as Node's `shell: true` does.
fn detached_spawn(cmd: &str, args: &[String], cwd: &str, shell: bool) -> Result<()> {
    let lower = cmd.to_ascii_lowercase();
    let bat = lower.ends_with(".cmd") || lower.ends_with(".bat");
    if shell || bat {
        // A 'command' item is the user's own shell text: verbatim. A .cmd/.bat
        // with arguments gets its words quoted (Node joined them unquoted, so a
        // path with spaces broke).
        let text = if shell {
            std::iter::once(cmd.to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ")
        } else {
            std::iter::once(cmd).chain(args.iter().map(String::as_str)).map(cmd_word).collect::<Vec<_>>().join(" ")
        };
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
        spawn_line(&format!("{} /d /s /c \"{text}\"", command_line(&comspec, &[])), cwd, true)?;
    } else {
        spawn(cmd, args, cwd, true)?;
    }
    Ok(())
}

/// A process Media tracks: a waiter thread reports its exit.
pub struct Tracked {
    pub pid: u32,
    // Shared with the waiter; the process object (and so its pid) stays
    // reserved until both are done with it, so kill never hits a reused pid.
    handle: Arc<OwnedHandle>,
}

impl Tracked {
    /// Ends the process (exit code 1, as Node's kill() gave). The exit is
    /// reported by the waiter like any other.
    pub fn kill(&self) {
        unsafe {
            let _ = TerminateProcess(HANDLE(self.handle.as_raw_handle()), 1);
        }
    }
}

/// Starts `exe` without a shell, window shown, and calls `on_exit(pid,
/// code)` from a waiter thread when it ends.
pub fn spawn_tracked(
    exe: &str,
    args: &[String],
    cwd: &str,
    on_exit: impl FnOnce(u32, Option<i32>) + Send + 'static,
) -> Result<Tracked> {
    let p = spawn(exe, args, cwd, false)?;
    let (pid, handle) = (p.pid, Arc::new(p.handle));
    let waiter = handle.clone();
    std::thread::spawn(move || {
        let h = HANDLE(waiter.as_raw_handle());
        let mut code = 0u32;
        let code = unsafe {
            WaitForSingleObject(h, INFINITE);
            GetExitCodeProcess(h, &mut code).ok().map(|_| code as i32)
        };
        drop(waiter);
        on_exit(pid, code);
    });
    Ok(Tracked { pid, handle })
}

// ---- The shell ----

fn shell_execute(file: &str, verb: PCWSTR) -> windows::core::Result<()> {
    let _com = Com::init();
    let file_w = wide(file);
    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb,
        lpFile: PCWSTR(file_w.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut sei) }
}

/// Electron's shell.openPath: the default action (files with their app,
/// folders in Explorer); a type with no app gets Windows' "Open with" dialog.
pub fn open_path(path: &str) -> Result<()> {
    match shell_execute(path, PCWSTR::null()) {
        Err(e) if e.code() == ERROR_NO_ASSOCIATION.to_hresult() => shell_execute(path, w!("openas")),
        r => r,
    }
    .map_err(|e| Error::msg(format!("Failed to open {path}: {}", e.message())))
}

/// Electron's shell.openExternal: a URL (or `ms-settings:`, `steam://`…)
/// with its registered handler.
pub fn open_external(url: &str) -> Result<()> {
    shell_execute(url, w!("open")).map_err(|e| Error::msg(format!("Failed to open {url}: {}", e.message())))
}

/// What a .lnk points at (Electron's readShortcutLink, the fields main.js used).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub target: String,
    pub args: String,
    pub cwd: String,
}

pub fn read_shortcut(path: &Path) -> Result<Shortcut> {
    let _com = Com::init();
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ctx("ShellLink")?;
    let file: IPersistFile = windows::core::Interface::cast(&link).ctx("ShellLink")?;
    unsafe { file.Load(PCWSTR(wide(path).as_ptr()), STGM_READ) }.ctx("Not a readable shortcut")?;
    let mut target = vec![0u16; 32768];
    let mut args = vec![0u16; 32768];
    let mut cwd = vec![0u16; 32768];
    unsafe {
        // Environment variables expanded, as Electron gave them; no Resolve(),
        // which can search the disk or show UI.
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_UNCPRIORITY.0 as u32).ctx("Shortcut target")?;
        link.GetArguments(&mut args).ctx("Shortcut arguments")?;
        link.GetWorkingDirectory(&mut cwd).ctx("Shortcut folder")?;
    }
    Ok(Shortcut { target: from_wide(&target), args: from_wide(&args), cwd: from_wide(&cwd) })
}

/// Electron's shell.showItemInFolder: an Explorer window on the parent
/// folder with the item selected (its parent folder opened if that fails).
pub fn show_in_folder(path: &Path) -> Result<()> {
    let _com = Com::init();
    let mut pidl = std::ptr::null_mut();
    let shown = unsafe { SHParseDisplayName(PCWSTR(wide(path).as_ptr()), None, &mut pidl, 0, None) }.and_then(|()| {
        // With no child items, the one item in pidl is selected in its parent.
        let r = unsafe { SHOpenFolderAndSelectItems(pidl, None, 0) };
        unsafe { ILFree(Some(pidl)) };
        r
    });
    match (shown, path.parent()) {
        (Ok(()), _) => Ok(()),
        (Err(_), Some(parent)) if parent.is_dir() => open_path(&parent.to_string_lossy()),
        (Err(e), _) => Err(e).ctx("Couldn't show that in Explorer"),
    }
}

// ---- Items ----

fn text<'a>(item: &'a Map<String, Value>, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Runs an apps.json item (or a Media entry that isn't a profile launch),
/// as lib/launcher.js's launch(entry).
pub fn launch_item(item: &Map<String, Value>) -> Result<()> {
    let (target, args, cwd) = (text(item, "target"), text(item, "args"), text(item, "cwd"));
    match text(item, "type") {
        // A plain open works for exes and folders alike; a resolved shortcut's
        // args or working folder need a real spawn.
        "path" if !args.is_empty() || !cwd.is_empty() => detached_spawn(target, &split_args(args), cwd, false),
        "path" => open_path(target),
        "stored" if target.is_empty() => Err(Error::msg("This stored item has no file.")),
        "stored" => open_path(target),
        "url" => open_external(target),
        "script" => {
            let interpreter = Some(text(item, "interpreter")).filter(|s| !s.is_empty()).unwrap_or("python");
            let all: Vec<String> = std::iter::once(target.to_string()).chain(split_args(args)).collect();
            detached_spawn(interpreter, &all, cwd, false)
        }
        "command" => detached_spawn(text(item, "command"), &[], cwd, true),
        other => Err(Error::msg(format!("Unknown launch type: {other}"))),
    }
}

/// `lnk --path P` (reads), `run --item <json>` (launches an apps.json-style
/// item), `show --path P` (Explorer with the item selected).
pub fn probe(args: &[String]) -> Result<Value> {
    let need = |name: &str| arg(args, name).ok_or_else(|| Error::msg(format!("{name} is required")));
    match args.first().map(String::as_str) {
        Some("lnk") => {
            let s = read_shortcut(Path::new(need("--path")?))?;
            Ok(json!({ "target": s.target, "args": s.args, "cwd": s.cwd }))
        }
        Some("run") => {
            let item: Value = serde_json::from_str(need("--item")?)?;
            let item = item.as_object().ok_or_else(|| Error::msg("--item must be a JSON object"))?;
            launch_item(item)?;
            Ok(json!({ "launched": item }))
        }
        Some("show") => {
            show_in_folder(Path::new(need("--path")?))?;
            Ok(json!({ "shown": need("--path")? }))
        }
        _ => Err(Error::msg("Usage: --probe launch lnk|run|show …")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::UI::Shell::CommandLineToArgvW;

    fn argv(line: &str) -> Vec<String> {
        let line_w = wide(line);
        let mut n = 0;
        let p = unsafe { CommandLineToArgvW(PCWSTR(line_w.as_ptr()), &mut n) };
        assert!(!p.is_null());
        // SAFETY: CommandLineToArgvW returned `n` valid strings in one LocalAlloc block.
        let out = (0..n as usize).map(|i| unsafe { (*p.add(i)).to_string().unwrap() }).collect();
        unsafe { LocalFree(Some(HLOCAL(p.cast()))) };
        out
    }

    #[test]
    fn command_lines_round_trip_through_command_line_to_argv() {
        let cases: &[&[&str]] = &[
            &["plain", "two words", ""],
            &["a\"b", "\"quoted\"", "trailing\\", "trail space\\", "\\\\server\\share\\x y\\"],
            &["back\\\\\"slash", "tab\there", "C:\\Program Files\\x\\", "日本 🎮", "--opt=a b"],
            &["\\", "\\\"", "\"\"", " "],
        ];
        for exe in ["C:\\Program Files\\mpv\\mpv.exe", "notepad.exe"] {
            for case in cases {
                let args: Vec<String> = case.iter().map(|s| s.to_string()).collect();
                let line = command_line(exe, &args);
                let mut expected = vec![exe.to_string()];
                expected.extend(args);
                assert_eq!(argv(&line), expected, "line: {line}");
            }
        }
    }

    #[test]
    fn split_args_matches_launcher_js() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("  -a  \"two words\" 'single q' x"), s(&["-a", "two words", "single q", "x"]));
        // Unclosed quotes fall to \S+, which then loses its leading quote.
        assert_eq!(split_args("\"abc def"), s(&["abc", "def"]));
        // \S+ runs through quotes in the middle of a word.
        assert_eq!(split_args("--x=\"a b\""), s(&["--x=\"a", "b"]));
        assert_eq!(split_args("\"\" ''"), s(&["", ""]));
        assert_eq!(split_args("a\u{3000}b"), s(&["a", "b"]));
    }

    #[test]
    fn cmd_words_are_quoted_when_cmd_would_split_them() {
        assert_eq!(cmd_word("C:\\Program Files\\x.cmd"), "\"C:\\Program Files\\x.cmd\"");
        assert_eq!(cmd_word("a&b"), "\"a&b\"");
        assert_eq!(cmd_word("plain"), "plain");
    }

    #[test]
    fn a_tracked_process_reports_its_exit_code_and_can_be_killed() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cmd = std::env::var("ComSpec").unwrap();
        let t = spawn_tracked(&cmd, &["/d".into(), "/c".into(), "exit 7".into()], "", {
            let tx = tx.clone();
            move |pid, code| tx.send((pid, code)).unwrap()
        })
        .unwrap();
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(), (t.pid, Some(7)));

        // ping to localhost: a harmless process that runs for a while.
        let ping = std::env::var("SystemRoot").unwrap() + r"\System32\PING.EXE";
        let args: Vec<String> = ["-n", "30", "127.0.0.1"].map(String::from).to_vec();
        let t = spawn_tracked(&ping, &args, "", move |pid, code| tx.send((pid, code)).unwrap()).unwrap();
        t.kill();
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(), (t.pid, Some(1)));
    }

    #[test]
    fn a_missing_program_is_an_error() {
        assert!(spawn("C:\\definitely\\not\\here.exe", &[], "", true).is_err());
        assert!(launch_item(json!({ "type": "nope" }).as_object().unwrap()).is_err());
        assert!(launch_item(json!({ "type": "stored", "target": "" }).as_object().unwrap()).is_err());
    }
}
