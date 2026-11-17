//! Starts library entries for Media and keeps track of the ones it can.
//! Profile launches (a player or emulator from profiles.json) are started
//! without a shell and kept, so Media knows when the game or film ends. The
//! other launch types go through the normal launcher like grid items and are
//! not tracked.
//!
//! Nothing here starts a process: that comes in through `Host`. Where the JS
//! listened for the child's 'exit' event and emitted its own, the host calls
//! `Launcher::exited` when the process ends and gets the `ExitInfo` back.
use serde::Serialize;
use serde_json::Value;

use super::library::Entry;
use super::players::{self, Launch, ProfilesData};
use crate::{Error, Result};

// A tracked process that ends this soon after starting most likely handed
// off to another process (Steam, a launcher stub, a single-instance player
// that passed the file to its open window), so its exit is not the end of
// the game. The page shows "launched" instead of "running" for these.
pub const HANDOFF_MS: i64 = 3000;

/// A process the host started for a tracked launch.
pub trait Child {
    fn pid(&self) -> u32;
    /// Ends the process. Its exit is reported through `Launcher::exited` like any other.
    fn kill(&mut self);
}

/// What the launcher needs from outside (the JS's injectable deps).
pub trait Host {
    type Child: Child;
    /// Starts `launch.exe` itself with `launch.args` as separate arguments,
    /// in `launch.cwd`: never through a shell, window not hidden, no stdio.
    /// A program that won't start is an `Err`.
    fn spawn(&mut self, launch: &Launch) -> Result<Self::Child>;
    /// Runs an entry that isn't a profile launch (path, url, script, command).
    fn launch_other(&mut self, entry: &Entry) -> Result<()>;
    fn exists(&self, path: &str) -> bool;
    /// Milliseconds, like Date.now().
    fn now(&self) -> i64;
}

/// What `launch` did. The JS shapes were `{ tracked: true, pid }`,
/// `{ tracked: false }` and `{ alreadyRunning: true }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launched {
    Tracked { pid: u32 },
    Untracked,
    AlreadyRunning,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExitInfo {
    pub id: String,
    pub code: Option<i32>,
    pub seconds: i64,
    pub handoff: bool,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: String,
    pub pid: u32,
    pub name: String,
    pub started_at: i64,
}

struct Run<C> {
    id: String,
    child: C,
    pid: u32,
    started_at: i64,
    name: String,
    stopped: bool,
}

pub struct Launcher<H: Host> {
    pub host: H,
    running: Vec<Run<H::Child>>, // in the order they started
}

impl<H: Host> Launcher<H> {
    pub fn new(host: H) -> Self {
        Launcher { host, running: Vec::new() }
    }

    /// Starts one entry. `profiles` is players::load()'s data. Fails with a
    /// message meant for the user.
    pub fn launch(&mut self, entry: &Entry, profiles: &ProfilesData) -> Result<Launched> {
        if self.is_running(entry.id()) {
            return Ok(Launched::AlreadyRunning);
        }
        let kind = entry.launch_type();
        if !entry.available() || (kind != "url" && kind != "command" && !self.host.exists(entry.target())) {
            let what = if entry.target().is_empty() { entry.name() } else { entry.target() };
            return Err(Error::msg(format!("Can't find {what}. Rescan the library or check the drive is connected.")));
        }
        if kind != "profile" {
            self.host.launch_other(entry)?;
            return Ok(Launched::Untracked);
        }
        let Some(profile) = players::profile_for(entry, profiles) else {
            let what = if entry.library() == "videos" { "videos" } else { entry.platform().unwrap_or("this game") };
            return Err(Error::msg(format!(
                "No player or emulator is set up for {what}. Add one in Settings, under Players and emulators."
            )));
        };
        let child = self.host.spawn(&players::build_launch(profile, entry.target())?)?;
        let pid = child.pid();
        self.running.push(Run {
            id: entry.id().to_string(),
            child,
            pid,
            started_at: self.host.now(),
            name: entry.name().to_string(),
            stopped: false,
        });
        Ok(Launched::Tracked { pid })
    }

    /// The host calls this when a tracked process has ended, with its exit
    /// code if it has one. None when `id` isn't running.
    pub fn exited(&mut self, id: &str, code: Option<i32>) -> Option<ExitInfo> {
        let run = self.running.remove(self.running.iter().position(|r| r.id == id)?);
        let ms = self.host.now() - run.started_at;
        Some(ExitInfo {
            id: run.id,
            code,
            seconds: (ms + 500).div_euclid(1000), // Math.round(ms / 1000)
            // Closed from Media: a quick exit is not a hand-off then.
            handoff: !run.stopped && ms < HANDOFF_MS,
            stopped: run.stopped,
        })
    }

    /// Ends a tracked launch (the Game running screen's Close game). Its
    /// exit follows as usual, through `exited`.
    pub fn stop(&mut self, id: &str) -> bool {
        let Some(run) = self.running.iter_mut().find(|r| r.id == id) else { return false };
        run.stopped = true;
        run.child.kill();
        true
    }

    pub fn list(&self) -> Vec<RunInfo> {
        self.running
            .iter()
            .map(|r| RunInfo { id: r.id.clone(), pid: r.pid, name: r.name.clone(), started_at: r.started_at })
            .collect()
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.iter().any(|r| r.id == id)
    }
}

// (e[key] || 0) + by, on whichever kind of number the file has.
fn bumped(e: &Entry, key: &str, by: i64) -> Value {
    match e.0.get(key) {
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => (i + by).into(),
            None => (n.as_f64().unwrap_or(0.0) + by as f64).into(),
        },
        _ => by.into(),
    }
}

/// Bumps an entry's launch count and last-launched time. Returns a new list;
/// unknown ids leave it unchanged.
pub fn record_launch(entries: &[Entry], id: &str, at: i64) -> Vec<Entry> {
    let mut next = entries.to_vec();
    for e in next.iter_mut().filter(|e| e.id() == id) {
        let count = bumped(e, "launchCount", 1);
        e.0.insert("launchCount".into(), count);
        e.0.insert("lastLaunched".into(), at.into());
    }
    next
}

/// Adds the length of a tracked session to an entry's play time. Hand-offs
/// (see HANDOFF_MS) don't count: their length says nothing about play time.
pub fn record_play(entries: &[Entry], id: &str, seconds: i64, handoff: bool) -> Vec<Entry> {
    let mut next = entries.to_vec();
    if handoff || seconds <= 0 {
        return next;
    }
    for e in next.iter_mut().filter(|e| e.id() == id) {
        let played = bumped(e, "playSeconds", seconds);
        e.0.insert("playSeconds".into(), played);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::test_entry;
    use serde_json::json;
    use std::cell::Cell;
    use std::rc::Rc;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const MPV: &str = "C:\\Program Files\\mpv\\mpv.exe";

    fn profiles() -> ProfilesData {
        players::sanitize(&json!({
            "profiles": [{ "id": "mpv", "name": "mpv", "kind": "player", "exe": MPV, "args": ["--fs", "{file}"], "platforms": ["videos"] }],
            "defaults": { "videos": "mpv" },
        }))
    }

    fn video(extra: Value) -> Entry {
        let mut e = json!({
            "id": ID,
            "library": "videos",
            "name": "Arrival",
            "platform": null,
            "type": "profile",
            "profile": null,
            "target": "D:\\Movies\\Arrival & Co (2016).mkv",
            "available": true,
        });
        for (k, v) in extra.as_object().unwrap() {
            e[k] = v.clone();
        }
        test_entry(e)
    }

    struct FakeChild {
        pid: u32,
        kills: Rc<Cell<u32>>,
    }

    impl Child for FakeChild {
        fn pid(&self) -> u32 {
            self.pid
        }
        fn kill(&mut self) {
            self.kills.set(self.kills.get() + 1);
        }
    }

    // A host stand-in: records calls; the test plays the part of the child.
    struct FakeHost {
        calls: Vec<Launch>,
        fail: Option<&'static str>,
        exists: bool,
        clock: i64,
        launched: Vec<String>,
        kills: Rc<Cell<u32>>,
    }

    impl Host for FakeHost {
        type Child = FakeChild;
        fn spawn(&mut self, launch: &Launch) -> Result<FakeChild> {
            self.calls.push(launch.clone());
            match self.fail {
                Some(message) => Err(Error::msg(message)),
                None => Ok(FakeChild { pid: 999 + self.calls.len() as u32, kills: self.kills.clone() }),
            }
        }
        fn launch_other(&mut self, entry: &Entry) -> Result<()> {
            self.launched.push(entry.target().to_string());
            Ok(())
        }
        fn exists(&self, _path: &str) -> bool {
            self.exists
        }
        fn now(&self) -> i64 {
            self.clock
        }
    }

    fn setup() -> Launcher<FakeHost> {
        Launcher::new(FakeHost { calls: Vec::new(), fail: None, exists: true, clock: 0, launched: Vec::new(), kills: Rc::default() })
    }

    fn error(launcher: &mut Launcher<FakeHost>, entry: &Entry, profiles: &ProfilesData) -> String {
        launcher.launch(entry, profiles).unwrap_err().to_string()
    }

    #[test]
    fn a_profile_launch_spawns_the_player_with_the_file_as_one_argument_and_no_shell() {
        let mut launcher = setup();
        let result = launcher.launch(&video(json!({})), &profiles()).unwrap();
        assert_eq!(result, Launched::Tracked { pid: 1000 });
        let call = &launcher.host.calls[0];
        assert_eq!(call.exe, MPV);
        assert_eq!(call.args, ["--fs", "D:\\Movies\\Arrival & Co (2016).mkv"]);
        // `opts.shell === false` has no counterpart: a Launch has no shell to
        // ask for, and Host::spawn's contract is to start the program directly.
        assert_eq!(call.cwd, "C:\\Program Files\\mpv");
    }

    #[test]
    fn a_tracked_launch_is_listed_while_running_and_reports_its_exit_with_the_time_played() {
        let mut launcher = setup();
        launcher.launch(&video(json!({})), &profiles()).unwrap();
        assert_eq!(launcher.list().iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), [ID]);
        assert!(launcher.is_running(ID));

        launcher.host.clock += 65_000;
        let exit = launcher.exited(ID, Some(0));
        assert_eq!(exit, Some(ExitInfo { id: ID.into(), code: Some(0), seconds: 65, handoff: false, stopped: false }));
        assert!(launcher.list().is_empty());
    }

    #[test]
    fn an_exit_right_after_starting_is_reported_as_a_hand_off() {
        let mut launcher = setup();
        launcher.launch(&video(json!({})), &profiles()).unwrap();
        launcher.host.clock += HANDOFF_MS - 1;
        assert!(launcher.exited(ID, Some(0)).unwrap().handoff);
    }

    #[test]
    fn launching_something_already_running_does_not_start_it_twice() {
        let mut launcher = setup();
        launcher.launch(&video(json!({})), &profiles()).unwrap();
        assert_eq!(launcher.launch(&video(json!({})), &profiles()).unwrap(), Launched::AlreadyRunning);
        assert_eq!(launcher.host.calls.len(), 1);
    }

    #[test]
    fn stop_ends_a_tracked_launch_and_its_exit_follows() {
        let mut launcher = setup();
        launcher.launch(&video(json!({})), &profiles()).unwrap();
        assert!(launcher.stop(ID));
        assert_eq!(launcher.host.kills.get(), 1);
        // The JS fake child emitted 'exit' from kill(); here the host reports it.
        let exit = launcher.exited(ID, None).unwrap();
        assert!(exit.stopped);
        assert!(!exit.handoff, "closing it quickly is not a hand-off");
        assert!(!launcher.stop(ID));
    }

    #[test]
    fn a_program_that_will_not_start_rejects_and_is_not_listed() {
        let mut launcher = setup();
        launcher.host.fail = Some("spawn ENOENT");
        assert!(error(&mut launcher, &video(json!({})), &profiles()).contains("ENOENT"));
        assert!(launcher.list().is_empty());
    }

    #[test]
    fn missing_files_and_unavailable_entries_are_refused_before_spawning() {
        let mut launcher = setup();
        launcher.host.exists = false;
        assert!(error(&mut launcher, &video(json!({})), &profiles()).contains("Can't find D:\\Movies"));
        assert!(error(&mut launcher, &video(json!({ "available": false })), &profiles()).contains("Can't find"));
        assert_eq!(launcher.host.calls.len(), 0);
    }

    #[test]
    fn no_profile_for_the_platform_gives_a_message_naming_it() {
        let mut launcher = setup();
        let game = video(json!({ "library": "games", "platform": "psx" }));
        assert!(error(&mut launcher, &game, &profiles()).contains("No player or emulator is set up for psx"));
        assert!(error(&mut launcher, &video(json!({})), &ProfilesData::default()).contains("set up for videos"));
    }

    #[test]
    fn other_launch_types_go_through_the_normal_launcher_and_are_not_tracked() {
        let mut launcher = setup();
        let steam = video(json!({ "type": "url", "target": "steam://rungameid/1145360" }));
        assert_eq!(launcher.launch(&steam, &profiles()).unwrap(), Launched::Untracked);
        assert_eq!(launcher.host.launched, ["steam://rungameid/1145360"]);
        assert_eq!(launcher.host.calls.len(), 0);
    }

    #[test]
    fn record_launch_and_record_play_update_one_entry_and_skip_hand_offs() {
        let entries = [video(json!({})), video(json!({ "id": "other" }))];
        let after = record_launch(&entries, ID, 50);
        assert_eq!(json!([after[0].0["launchCount"], after[0].0["lastLaunched"]]), json!([1, 50]));
        assert_eq!(after[1], entries[1]);
        assert!(!entries[0].0.contains_key("launchCount"), "input not mutated");

        let played = record_play(&after, ID, 30, false);
        assert_eq!(record_play(&played, ID, 10, false)[0].0["playSeconds"], 40);
        assert_eq!(record_play(&after, ID, 1, true), after);
    }

    // Stands in for the player in the test below (the JS used node.exe): run
    // as a child of this test binary, it exits 7 when its last argument is
    // the file it was told to expect. Does nothing in a normal test run.
    #[test]
    fn player_stand_in() {
        if let Ok(expect) = std::env::var("LAUNCHPAD_TEST_EXPECT") {
            std::process::exit(if std::env::args().next_back() == Some(expect) { 7 } else { 1 });
        }
    }

    #[cfg(windows)] // a profile's program must be an .exe
    #[test]
    fn a_real_process_is_spawned_tracked_and_reported_when_it_exits() {
        use std::process::{Command, Stdio};
        use std::sync::{Arc, Mutex};

        type Shared = Arc<Mutex<Option<std::process::Child>>>;
        struct RealChild(u32);
        impl Child for RealChild {
            fn pid(&self) -> u32 {
                self.0
            }
            fn kill(&mut self) {}
        }
        struct RealHost {
            child: Shared,
            expect: String,
        }
        impl Host for RealHost {
            type Child = RealChild;
            fn spawn(&mut self, launch: &Launch) -> Result<RealChild> {
                let child = Command::new(&launch.exe)
                    .args(&launch.args)
                    .current_dir(&launch.cwd)
                    .env("LAUNCHPAD_TEST_EXPECT", &self.expect)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                let pid = child.id();
                *self.child.lock().unwrap() = Some(child);
                Ok(RealChild(pid))
            }
            fn launch_other(&mut self, _entry: &Entry) -> Result<()> {
                unreachable!()
            }
            fn exists(&self, path: &str) -> bool {
                std::path::Path::new(path).exists()
            }
            fn now(&self) -> i64 {
                crate::time::now_ms()
            }
        }

        let dir = crate::test_util::temp_dir();
        let file = dir.path().join("clip & more.mkv");
        std::fs::write(&file, "").unwrap();
        let file = file.to_string_lossy().into_owned();
        let this_exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        let stand_in = players::sanitize(&json!({
            "profiles": [{
                "id": "stand-in",
                "name": "stand-in",
                "kind": "player",
                "exe": this_exe,
                "args": ["--exact", "media::launch::tests::player_stand_in", "{file}"],
                "platforms": ["videos"],
            }],
            "defaults": { "videos": "stand-in" },
        }));
        let child: Shared = Arc::default();
        let mut launcher = Launcher::new(RealHost { child: child.clone(), expect: file.clone() });
        let result = launcher.launch(&video(json!({ "target": file })), &stand_in).unwrap();
        assert!(matches!(result, Launched::Tracked { .. }));
        let status = child.lock().unwrap().take().unwrap().wait().unwrap();
        let info = launcher.exited(ID, status.code()).unwrap();
        assert_eq!(info.code, Some(7));
        assert!(info.handoff);
    }
}
