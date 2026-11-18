# Launchpad 1.3: Rust Port Plan

Written 2026-11-16 against v1.2.2. Goal: Launchpad 1.3, the same app as one native Rust process, using a fraction of the memory and disk. `CODEBASE_SUMMARY.md` stays the reference for what each feature does; this file says how it moves.

## 1. Where we are (measured)

Dev build of v1.2.2 running on this PC (Windows 10 19045), main window open:

| | Today |
|---|---|
| Processes | 4 × Electron (browser, GPU, renderer, utility) + PowerShell helpers |
| Memory, Electron only | **~370 MB** working set, 225–300 MB private across two readings. The 76–111 MB seen in Task Manager is one process each, not the total. |
| Memory, each PowerShell helper | ~65 MB private (`windowmgr.ps1` stays running while the organizer or "disable hotkey in apps" is on) |
| Every extra window | The panel (one per display), overlay, grid placer, scenes menu, window picker and Media are each a Chromium renderer process |
| Installer | 112 MB (`Launchpad-Setup-1.2.0.exe`) |
| Installed | 371 MB (`Launchpad.exe` alone is 246 MB) |
| Tray widget action | One `powershell.exe` start per call, 200–500 ms each |
| Code | ~22,400 lines: 7,700 main/lib JS, 1,300 PowerShell/C#, 9,700 renderer HTML/CSS/JS, 3,700 tests |

## 2. Targets

These are the gates. Phase 0 proves they are reachable before any real porting; if a number can't be met, it is changed here with the measurement that says why.

| | Target | vs today |
|---|---|---|
| Processes | 1, no helpers | 5+ |
| Private memory, hidden in tray | ≤ 15 MB | 225–300 MB |
| Private memory, main window open (200 items, wallpaper) | ≤ 40 MB | 225–300 MB + helpers |
| Private memory, organizer on, 2 displays, main window hidden | ≤ 25 MB | not measured (more: extra renderers + helper) |
| Private memory, Media open with art | ≤ 80 MB | not measured |
| `Launchpad.exe` | ≤ 12 MB (was 8; see Phase 0 result) | 246 MB |
| Installer | ≤ 5 MB (was 4) | 112 MB |
| Cold start to window | ≤ 150 ms | ~1–2 s |
| Hotkey to visible | ≤ 50 ms | |
| Tray widget action (volume, Wi-Fi list…) | ≤ 20 ms, no process start | 200–500 ms |
| Idle CPU, hidden | 0 timers firing | polls |

## 3. The one big decision: the UI

**Recommendation: native UI with [Slint](https://slint.dev), software renderer. Not Tauri.**

- **Tauri / WebView2** would keep the 9,700 lines of HTML/CSS/JS and shrink the installer to a few MB, but the UI would still be Chromium (`msedgewebview2.exe`, 100–200 MB while any window exists), and the organizer panel is always on screen. It fixes size and misses the memory goal. Rejected for that reason alone.
- **Slint** is a retained, declarative toolkit (`.slint` markup, close in shape to the current HTML/CSS), compiled into the binary. Its software renderer needs no GPU context, which is where most of a small GUI app's memory goes, and it repaints only dirty regions. The renderer is a build feature, so switching to its OpenGL one later changes no UI code.
- **egui** was the other candidate. Easier freeform canvas, but its OpenGL/wgpu context costs 40–100 MB on Windows, and its text shaping is weaker for non-Latin window titles.
- **Raw Win32 + Direct2D** would be smallest of all, at the price of writing our own widgets (text fields, lists, sliders, colour pickers). Not worth it unless Slint fails Phase 0.

The cost is honest: the renderer (HTML/CSS/JS) is rewritten, not ported. The main-process logic and the tests port almost line for line.

### Phase 0 result (2026-11-16): go, with Slint's software renderer for every window

Measured on this PC (2560×1440 at 100%, Slint 1.19.0, the release profile in §4.3). Code in `spikes/rust/`.

| | Gate | Software renderer | OpenGL (femtovg) |
|---|---|---|---|
| Main window (200 tiles, wallpaper, bar), 1280×800, private | ≤ 40 MB | **20.2 MB** | 154.6 MB |
| Same, maximized 2560×1440 | ≤ 40 MB | **29.9 MB** | 168.0 MB |
| Panel alone (docked AppBar), private | ≤ 15 MB | **2.8 MB** | 119 MB |
| Panel + full-screen overlay | | 16.7 MB | 109 MB |
| Tile drag / scroll frame time | ≤ 16.7 ms | ~7 ms (paced at ~150 fps) | ~7 ms |
| Idle CPU, panel with a clock | ~0 | 0 | |
| Exe: main window spike / shell windows spike | ≤ 8 MB | 7.87 MB / 6.84 MB | 7.90 / 7.42 MB |

What it settled:
- **Per-pixel transparency works** with the software renderer and the ordinary winit backend. Panel and overlay blend correctly (checked by reading pixels back), with clean anti-aliased corners. Neither fallback is needed. The OpenGL renderer is out: 5–40× the memory, and its panel came out opaque.
- **Shell window behaviour works:** clicking the panel leaves the foreground alone, the overlay is click-through, the AppBar docks and the work area comes back on exit, from the panic hook, and from a later process after a hard kill (`ABM_REMOVE` on the stale handle).
- **Text:** Japanese, Cyrillic and accented names render through system font fallback, nothing bundled. **Emoji are monochrome outlines**, not colour. Accepted for now.
- **Exe gate raised from 8 to 12 MB.** Slint itself is ~7 MB before any app code. Still 20× smaller than today. To claw some back if wanted: `opt-level = "z"`, and decoding images through WIC so Slint's own decoders can be left out.

What it adds to the design:
- **The overlay's buffer is ~14 MB per display and only freed by `hide()` then dropping the component.** So overlays, the grid placer and the main window are created on show and dropped (after `hide()`) on hide, as §4.3 already says. Dropping without hiding leaks the window.
- **Window styles are applied after the first show**, once the HWND exists (`window_handle()`), with a winit attributes hook (`with_active(false)`, `with_skip_taskbar(true)`) so the window doesn't take the foreground as it appears. winit sets `WS_EX_APPWINDOW`, which must be cleared. winit may rewrite extended styles when its own flags change: re-apply ours after any such change, and test for it.
- **AppBar notifications** (`ABN_POSCHANGED`, `ABN_FULLSCREENAPP`) need a window subclass; winit drops them.
- Shell windows need `run_event_loop_until_quit()`: the default loop ends when the last window hides.
- No virtualisation outside `ListView`: all tiles of the open island are live elements. Fine at 200; check at 1,000.
- Popups are drawn inside their window and clipped at its edge: tray popouts and context menus need flip logic, as today.

Not checked in Phase 0, carried into Phase 3/4: scaling above 100%, IME and clipboard in text fields, wheel scrolling by hand, a few 30–75 ms hitches seen in animated scrolling (cause unknown), Alt+Tab absence (inferred from style bits), the pre-blurred bar.

**Licence:** Slint is GPLv3, or free for desktop apps under its royalty-free licence with attribution (an "About" line). The repo has no licence file today. Decide before Phase 3.

## 4. Architecture

One process, one UI thread running the Slint event loop and the Win32 message loop, plus short-lived worker threads for anything slow (file copy, scan, HTTP, icon decode) that post results back with `slint::invoke_from_event_loop`. No async runtime.

```
Cargo.toml                 workspace
crates/core/               pure logic, no Windows or UI. `cargo test` runs here in seconds.
crates/app/src/win/        Win32, COM and WinRT through the `windows` crate
crates/app/src/ui/         glue between core, win and the Slint models
crates/app/ui/*.slint      the windows
installer/launchpad.nsi    NSIS script
```

The Electron code stays at the repo root, runnable, until Phase 7: it is the reference build that parity is tested against.

### 4.1 What goes away

- **IPC, preloads, CSP, sandbox, `decorate()`**: ~110 channels become plain method calls. Icons are no longer base64 data URLs sent with every list.
- **Every PowerShell helper and the inline C#**, including `windowmgr.ps1`, `appbar-guard.ps1`, `unpacked-path.js` and the JSON-lines protocol. The helper's 5 s timeout, restart logic and `helperExit` handling go with it.
- **`lpmedia:` protocol, `favicon.js` ICO decoder** (WIC decodes ICO), `fix-exe-icon.js`, `scripts/build.js`, `electron-updater`.
- **Known issue §11.2** (garbled non-ASCII from PowerShell): gone by construction, everything is UTF-16 from the API.

### 4.2 Module map

| Today | Rust | Notes |
|---|---|---|
| `lib/store.js`, `layout.js`, `defaults.js`, `settings.js`, `safe-file.js`, `reset.js`, `file-store.js` | `core::{store, layout, defaults, settings, safe_file, reset, file_store}` | Straight port. `serde_json`, unknown keys kept with `#[serde(flatten)]`. |
| `lib/zones.js`, `layouts.js`, `strip.js`, `autohide.js`, `picker.js`, `scenes.js`, `fullscreen.js`, `renderer/cells.js` | `core::organizer::*` | Straight port, already pure and tested. |
| `lib/media/{library,players,scan,controller,backup}.js`, `renderer/media/{pad,theme}.js` | `core::media::*` | Straight port. |
| `lib/favicon.js` (URL + `<link>` parsing) | `core::favicon` | ICO decoding dropped. |
| `lib/windowmgr.ps1` + `.js` | `win::windowmgr` | `SetWinEventHook`, `SetWindowPos` on DWM frame bounds, `SHAppBarMessage`, fullscreen walk. Same logic, in-process, events are function calls. |
| `appbar-guard.ps1`, `appbars.json` | `win::appbar` | Keep `appbars.json` + release-on-start (covers kill and crash). Add a panic hook and `SetUnhandledExceptionFilter` that remove AppBars. The separate guard process is dropped; a hard kill is healed on next start, as the killed-tree case already is. |
| `volume.ps1`, `audio-devices.ps1` | `win::audio` | Core Audio COM (`IAudioEndpointVolume`, `IAudioSessionManager2`, `IPolicyConfig`). Change callbacks replace polling. |
| `wifi.ps1` (netsh) | `win::wifi` | Native Wifi API (`WlanGetAvailableNetworkList`, `WlanConnect`). No output parsing. |
| `bluetooth.ps1` (`Get-PnpDevice`) | `win::bluetooth` | SetupAPI / CfgMgr32 for the same BTHENUM/BTHLE devices and the same disable-device "disconnect". |
| `drives.ps1` | `win::drives` | `GetLogicalDrives` + `IOCTL_STORAGE_QUERY_PROPERTY` for USB; eject keeps the Shell "Eject" verb. `WM_DEVICECHANGE` replaces polling. |
| `power.ps1` | `win::power` | `InitiateSystemShutdownEx`, `SetSuspendState`. |
| `windows.ps1` | `win::windowmgr` | Same enumeration as scenes already uses. |
| `lib/icons.js`, `nativeImage` | `win::image` | `IShellItemImageFactory` for exe icons and thumbnails, WIC for decode, resize, PNG/JPEG encode. No image crate. |
| `lib/launcher.js`, `shell.*` | `win::launch` | `ShellExecuteExW`, `CreateProcessW`; `.lnk` through `IShellLinkW`. |
| `net.fetch`, `reinstall.js`, `electron-updater` | `win::http`, `core::update` | WinHTTP (the TLS stack stays in Windows, not in the exe), SHA-512 through CNG. One code path for update and reinstall: what `reinstall.js` does today. |
| `globalShortcut`, `Tray`, `Menu`, `dialog`, `Notification`, single instance, auto-launch | `win::{hotkey, tray, dialogs, notify, instance, autostart}` | `RegisterHotKey`, `Shell_NotifyIconW`, `IFileOpenDialog`, named mutex + window message, `HKCU\…\Run`. |
| `screen.*` | `win::display` | `EnumDisplayMonitors`, per-monitor-v2 DPI for the whole process. Everything in physical pixels, so `dipToScreenRect` conversions disappear. |
| `main.js` handlers | `app::ui::*` | Thin: call core, update the Slint model. |
| `renderer/*` (main window) | `ui/main.slint` + parts | **Rewrite.** Grid canvas, islands, taskbar, tray popouts, search, context menus, modals, settings. |
| `renderer/{panel,overlay,grid,scenes-menu,window-picker}.*` | `ui/*.slint` | **Rewrite**, small. Window styles (`WS_EX_NOACTIVATE`, tool window, click-through, topmost) set on the HWND. |
| `renderer/media/*` | `ui/media.slint` | **Rewrite.** Spatial focus (`focus.js`) ports as logic. Gamepad through XInput; see risks. |

### 4.3 How the memory target is met

1. **No Chromium, no Node, no PowerShell.** That alone is the bulk of it.
2. **Windows exist only while shown.** The main window, Media, grid placer, picker and scenes menu are created on show and dropped on hide; state lives in the model, not the window. Panels exist only for displays that have one.
3. **Images are decoded at the size they're drawn and cached with a byte cap.** Icons at tile size, the wallpaper once at window size (dropped when hidden), Media art at tile size in an LRU. Today every `apps:list` reply carries every icon as base64.
4. **Events, not polls.** Audio, device and Wi-Fi changes arrive as callbacks. The clock timer runs only while a clock is visible. Cursor polling stays only where it is today (during a window drag, auto-hide panel) and stops otherwise.
5. **Small dependency set.** `slint`, `windows`, `serde`, `serde_json`. No tokio, no reqwest, no `image`. UUIDs from `CoCreateGuid`.
6. **Release profile:** `opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
7. **Budgets are enforced.** `.claude/skills/test-launchpad/measure.ps1` prints private bytes and sizes; CI fails the build if the exe or installer is over its target.

### 4.4 Data and upgrade compatibility

- **1.3 is a manual download; every release after it updates itself from inside the app**, as 1.2 did (decided 2026-11-17). 1.2.2's Settings already tells people to get 1.3 from GitHub Releases.
- Same folder (`%APPDATA%\launchpad`), same files, same JSON. 1.3 must read every file a 1.2.2 install wrote, and a backup written by either must import in the other. Checked by fixture tests in `crates/core`.
- **Visual prefs live in Chromium's localStorage today** (§4.3 of the summary: theme, accent, wallpaper, bar layout, tray order, Media colours). There is no later Electron release to hand them over, so on its first run 1.3 reads them once from Chromium's `Local Storage\leveldb` in the data folder (keys under the `file://` origin; the `.log` file plus any `.ldb` tables, which need a small Snappy decoder) and writes `ui.json`, which it uses from then on. Best effort: if anything about it fails, 1.3 starts with default looks and nothing else is affected. Phase 3 task, tested against a copy of a real 1.2.2 profile.
- After a first run that has written `ui.json`, 1.3 deletes the Chromium folders in the data folder (`Cache`, `GPUCache`, `Local Storage`, …). They are caches, usually tens of MB.
- **Installing 1.3 over 1.2.x:** the person runs `Launchpad-Setup-1.3.0.exe` by hand. It finds the Electron install (its uninstall registry entry), quits a running Launchpad, runs the old uninstaller silently (files only, never the data folder) and installs to the same place.
- **Updates from 1.3 on** (`crates/core/src/update.rs`): each release carries the installer and `launchpad-update.yml` (electron-builder's `latest.yml` format: `version`, `path`, `sha512`). The app checks it, offers the update in Settings and optionally as a notification (`notifyUpdates`, as in 1.2), downloads the installer, checks its SHA-512, runs it with `/S --force-run` and quits. "Reinstall this version" is the same path aimed at the running version's tag. No `.blockmap`: the installer is a few MB.
- **The manifest is not called `latest.yml`**, so 1.2.2's electron-updater never offers 1.3 (it reads that name from the newest release). The cost: a manual "Check for updates" in 1.2.2 shows an error once 1.3 is the newest release, which its Settings note explains.
- "Start with Windows": remove the Run value Electron wrote, write our own if it was on. Check the old value's name on a real install.
- Controller maps are keyed by Chromium's Gamepad id strings and button numbers. They won't match XInput; they reset to defaults once. Dead zone carries over.

## 5. Phases

Each phase ends with something that runs. Sizes are relative.

**Phase 0 — Spike and gates (S). Done, passed; result in §3.** Throwaway code in `spikes/rust/`. Build: (a) a window with 200 icon tiles on a wallpaper, a translucent bar, a text field and a context menu; (b) a transparent, non-activating, topmost 32 px panel docked as an AppBar; (c) a click-through transparent overlay. Measure with `measure.ps1`. Pass if: private memory ≤ 40 MB with (a) open and ≤ 15 MB with only (b); exe ≤ 8 MB; per-pixel transparency works on (b) and (c); a window title in Japanese and one with an emoji render; dragging a tile holds 60 fps on the 2560×1440 display. If transparency fails in the software renderer, try in order: Slint's OpenGL renderer for those windows; drawing them into our own buffer and `UpdateLayeredWindow`. Outcome is written into §2 and §3 here.

**Phase 1 — Core (M).** `crates/core`: every pure module in §4.2, with the 26 `test/*.test.js` files ported one for one, plus fixture tests (real `apps.json`, `settings.json`, `layouts.json`, `library.json`, `profiles.json`, backups v2, v3 and the bare-array form). Done when `cargo test` passes and round-trips the fixtures byte-compatibly enough that 1.2.2 reads them back.

  **Done 2026-11-17.** 297 tests (290 unit, 7 compatibility); `cargo clippy` clean; dependencies are `serde` and `serde_json` only. Fixtures in `crates/core/tests/fixtures/` are made by the real 1.2.2 modules (`node crates/core/tests/fixtures/generate.js`). A 1.2.2 data folder loads and saves back byte for byte; all three backup forms import as they do in 1.2.2; a Rust export matches 1.2.2's byte for byte and imports in the real JS. Differences left, all only reachable by hand-editing a file, none losing data the app uses:
  - Numbers a person typed as `2.0`, `1e3` or `-0` are written back in that form where 1.2.2 writes `2`, `1000`, `0`.
  - A JSON list sitting among the entries of `apps.json` is dropped (1.2.2 keeps it; nothing can show or use it).
  - A string with half a UTF-16 surrogate pair makes the file unreadable to serde_json: it's set aside and the app goes read-only or restores the backup, as for any corrupt file.
  - Name sorting uses code-point order where 1.2.2 used the locale's (ties among non-ASCII names in the one-time pin-order migration and the Add folder preview).
  Phase 2 supplies what core takes as traits: HTTP fetch, SHA-512, image decode/resize/encode, thumbnails and file icons, and process spawning for Media.

**Phase 2 — Windows layer (L).** `crates/app/src/win`, one module at a time, each with a `launchpad.exe --probe <area>` debug command that prints JSON (list audio devices, list windows, list Wi-Fi…). Probes are read-only unless given an explicit action. Done when every row of the PowerShell table in summary §6.2 has a probe returning the same data as the `.ps1`, with non-ASCII names intact.

  **Done 2026-11-17.** `crates/app` (binary `launchpad`, 5,500 lines, 43 tests; workspace 340 tests, clippy clean). Release exe 0.54 MB with the whole layer in. Every PowerShell helper has a `--probe` that returns the same data, compared on this PC: window lists, placing (0 px error on both monitors, 6 cases), fullscreen detection and the foreground/drag event streams identical to `windowmgr.ps1`; audio devices, volume and sessions identical; all 16 Wi-Fi networks identical; Bluetooth and USB drives identical (both empty here; the Bluetooth status rule was also checked over all 241 devices). Non-ASCII titles arrive intact. A probe answers in ~10–60 ms where the `.ps1` took ~250 ms. Core's traits are implemented for real (WinHTTP, WIC, CNG, process launching) and exercised: favicons from three live sites, SHA-512 equal to `certutil`, launches of every item type.
  Never run on this PC, by design (they change the machine): shutdown, restart, sleep, empty Recycle Bin, Wi-Fi connect/disconnect, Bluetooth disconnect, eject, changing the default audio device, writing the real Run key (`migrate_from_electron`). Not reachable here: elevated windows, mixed DPI, Explorer restart, a hard kill with the panel docked, the crash handlers, tray clicks (the taskbar auto-hides). These carry into the Phase 3/4 feature runs.
  Learned, for Phases 3–5:
  - Everything that blocks (HTTP, shell images and thumbnails, shortcuts, opening files, dialogs, popup menus) runs off the UI thread or, for dialogs and menus, holds no `RefCell` borrow across the call. Callbacks from audio, Wi-Fi and window events arrive on other threads and only post to the UI thread.
  - Tray, hotkeys and single instance share one hidden top-level window (not `HWND_MESSAGE`: that gets no `TaskbarCreated` broadcast); `WM_DEVICECHANGE` needs a top-level window too.
  - Bluetooth "disconnect" disables the device, which needs administrator rights, so it has never worked in the unelevated 1.2 app either. Phase 3 decides whether to keep it.
  - 1.2's Sleep went through `rundll32`, which drops its arguments and may hibernate; 1.3 really sleeps.
  - WinHTTP's own receive timeout fired ~4× late; each request has a watchdog that closes it at the deadline.

**Phase 3 — Main window (L, the biggest).** Tray, hotkey, single instance, then the grid (tiles, islands, drag, marquee, keyboard), taskbar (pins, open windows, search, recent), tray widgets and popouts, context menus, add/edit and the other modals, Settings, themes and bar styles, Explorer drops and the file store, backup, reset. Done when the skill's sections L, G, T, W, P and D pass.

**Phase 4 — Organizer (M).** Panel, AppBar docking, auto-hide, fullscreen hiding, drag strip and overlay, grid placer, window picker, Snap to menu, layouts editor, scenes and the scenes menu, top bar hosted on the panel. Done when sections O and S pass, including the open items from summary §11.3 that this PC can test.

**Phase 5 — Media (M).** Window, spatial focus, gamepad, library screens, add folder/app, profiles, tracked launches, art, colours, controller setup. Done when section M passes.

**Phase 6 — Packaging and updates (S).** `installer/launchpad.nsi`, updater, release workflow (`cargo build --release` → `makensis` → `latest.yml` → `gh release create`), `test.yml` running `cargo test` plus the size gate. Done when: installing 1.3 by hand over a 1.2.2 install keeps its data, autostart and hotkey; a 1.3.0 install updates itself from inside the app to a test 1.3.1 release; 1.2.2 does not offer 1.3; an uninstall leaves nothing but the data folder.

**Phase 7 — Parity pass and cut-over (S).** Full run of the testing skill on both builds side by side, fix what differs, check every budget in §2, then delete the Electron tree (`main.js`, `lib/`, `renderer/`, preloads, `scripts/`, `package*.json`, `test/`), rewrite `CODEBASE_SUMMARY.md` and the README's build section, release 1.3.0 (a manual download; see §4.4).

Phases 3, 4 and 5 don't depend on each other once 2 is done.

## 6. Testing plan

One checklist, in `.claude/skills/test-launchpad/features.md`, is the list of everything Launchpad does. It is not repeated here. The `test-launchpad` skill says how to run it.

| Layer | What | When |
|---|---|---|
| 1. Unit | `cargo test -p launchpad-core`: the ported tests and the data fixtures | Every push (CI) |
| 2. Probes | `--probe` output per system area compared with what Windows itself shows | End of Phase 2, then each release |
| 3. Features | The checklist, driven through the real UI with a throwaway data folder. Run on Electron 1.2.2 first to record what "correct" looks like, then on Rust. A row passes when both behave the same, or the difference is written down as intended. | End of Phases 3, 4, 5; full run in 7 |
| 4. Budgets | `measure.ps1` in each state of §2; size gate in CI | Phase 0, then every phase |
| 5. Upgrade | 1.2.2 with real data → install 1.3 by hand → every data file intact, looks carried over, autostart and hotkey still work; 1.3.0 → in-app update to a test 1.3.1; backup from one build imports in the other | Phase 6 |
| 6. Soak | Organizer on, 24 h of normal use: memory flat, no leaked AppBar space after quit, kill and crash | Phase 7 |

Rows that change the machine (shutdown, sleep, eject, Bluetooth and Wi-Fi disconnect, empty Recycle Bin, reset, installing an update) are tested up to their confirmation, then completed only with a person saying yes. Rows that need hardware this PC may not have (a real controller, monitors at different scaling, an elevated window, Windows 11 Snap Layouts) are marked and listed in the run's report rather than skipped silently; most are already untested today (summary §11).

## 7. Risks

| Risk | Plan |
|---|---|
| ~~Slint's software renderer can't do per-pixel transparent windows~~ | Retired by Phase 0: it can. |
| winit rewrites the extended window styles we set (no-activate, tool window, click-through) | Re-apply after show and after any winit flag change; a test clicks the panel and checks the foreground didn't move. |
| Exe grows past 12 MB as the app fills in | Size gate in CI from the first real commit, so the dependency that does it is obvious. |
| Blurred translucent bars (`backdrop-filter: blur(20px)`) have no equivalent | Blur the strip of wallpaper under the bar once, when the wallpaper or bar changes, and draw that. |
| Emoji in titles are monochrome | Non-Latin text passed Phase 0. Colour emoji would mean drawing them as images; not planned unless it bothers in use. |
| Dragging between windows (grid item onto the panel's breadcrumb) | Both windows are ours and in one process: track the drag in the app, hit-test the panel by cursor position. No OLE needed. Explorer drops in use the standard window drop target. |
| Gamepad: XInput covers Xbox-style pads only | Start with XInput. If a non-XInput pad matters, add `Windows.Gaming.Input` raw controllers behind the same `PadReader`. |
| `IPolicyConfig` (set default audio device) is undocumented | It is what `audio-devices.ps1` already uses; same GUIDs. |
| The rewrite drifts from the original's behaviour | The Electron build stays runnable until Phase 7 and layer 3 compares against it. |
| Losing AppBar space on a crash without the guard process | Panic hook + exception filter + release-on-start. Soak test kills the process on purpose. |

## 8. Open questions

1. Slint under its royalty-free licence (attribution in About), or make the repo GPLv3?
2. Keep per-user install only, or keep the installer's per-machine choice?
