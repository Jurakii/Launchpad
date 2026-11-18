---
name: test-launchpad
description: Test Launchpad's features in the real running app, on the Rust build, the Electron build, or both side by side for parity. Use when asked to test Launchpad, verify a feature or a phase of the Rust port, check for regressions before a release, compare the two builds, or measure Launchpad's memory and install size.
---

# Testing Launchpad

`features.md` (next to this file) lists every feature with what must be true. This file says how to run it safely. `RUST_PORT_PLAN.md` §6 says when each layer is due.

## Rules

1. **Never test against the person's real data.** Always pass a throwaway data folder (below). If the app didn't write `apps.json` into that folder within a few seconds of starting, the flag was ignored: quit it and stop.
2. **[confirm] rows stop at the confirmation.** Shutdown, restart, sleep, eject, Bluetooth and Wi-Fi disconnect, changing the default audio device, Empty Recycle Bin, installing or reinstalling, restarting Explorer: check the UI up to the question, then ask the person. Reset (D11) may run without asking only because the data folder is throwaway.
3. **[hardware] rows that can't run are reported as "not run: <what's missing>"**, never as passed and never left out.
4. **A Launchpad that's already running is the person's.** Don't close it without asking. The test copy can run beside it because it uses another data folder and other hotkeys.
5. **Leave the PC as found:** close every Notepad and test window you opened, quit the test copy, check that a maximized window fills the whole screen again (no leftover panel space), delete the throwaway folder.
6. Report what you saw, row by row. A row you didn't run is not a pass.

## Setup

Make the data folder and seed hotkeys that can't collide with a running copy (PowerShell):

```powershell
$data = Join-Path $env:TEMP "launchpad-test-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -ItemType Directory -Force $data | Out-Null
[IO.File]::WriteAllText("$data\settings.json", '{"hotkey":"CommandOrControl+Alt+F11","organizerHotkey":"CommandOrControl+Alt+F12","notifyUpdates":false}')
```

Write seeded JSON without a BOM, as above. Windows PowerShell's `Set-Content -Encoding utf8` adds one, and both builds then set the file aside as corrupt (`settings.corrupt-….json`), so the test runs on default settings, including the real Ctrl+Space hotkey.

For X1 (first run) use an empty folder instead. For D7 (old data) copy a real 1.2.2 data folder into it, never point at the original.

Start the build under test, in the background:

| Build | Command | Process name |
|---|---|---|
| Rust | `cargo run --release -p launchpad -- --user-data-dir="$data"` | `launchpad` |
| Electron (reference, until it's deleted) | `npm start -- --user-data-dir="$data"` | `electron` |

Both builds take the same `--user-data-dir` flag on purpose. If the Rust build doesn't accept it yet, that's the first bug to report.

## The layers, cheapest first

Stop and report at the first layer that fails badly; later layers aren't worth running on a broken build.

1. **Unit tests.** `cargo test -p launchpad-core` (Rust) or `npm test` (Electron). All must pass.
2. **Probes (Rust only).** `launchpad.exe` is a GUI exe, so Windows PowerShell doesn't wait for it or show its output unless it's piped: `& $exe --probe windows | Out-String` (Bash waits on its own). `launchpad.exe --probe <area>` prints JSON for `audio`, `windows`, `wifi`, `bluetooth`, `drives`, `displays`. Compare with what Windows shows (Settings, Task Manager, `Get-Volume`, `netsh wlan show interfaces`). Open a Notepad titled with `Café 日本` and check the title survives.
3. **Features.** Work through the sections of `features.md` that were asked for (all of them for a release or the Phase 7 parity pass).
   - Drive the UI with computer use: the Rust UI is native, so there's no page to script. Take a screenshot before asserting anything about what's on screen.
   - Prefer checking the result outside the UI: read `apps.json`, `settings.json`, `layouts.json` in `$data`; look for the marker file a launched script wrote; `Get-Process notepad`; a window's position from a screenshot.
   - Use Notepad as the subject window for T, O and S. Use a temp folder with a few small `.txt`, `.png` and empty `.sfc`/`.mp4`-named files for F and M.
   - For a parity run, do each section on Electron first and note what happens, then on Rust. Same result, or the difference is written down in the report.
4. **Budgets (section B).** Reach the state, wait 30 s, then:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\test-launchpad\measure.ps1 -Name launchpad -Path target\release\launchpad.exe,dist\Launchpad-Setup-1.3.0.exe
   ```

   `totalPrivateMB` is the number the targets use; `processCount` must be 1 for the Rust build. Use `-Name electron` for the reference numbers. For B2, seed 200 items by writing them into `apps.json` before starting rather than adding them by hand.

## Things that go wrong

- **Panel space not given back** after a test copy is killed: start the test copy again with the same data folder and quit it normally; it releases what `appbars.json` lists.
- **Hotkey does nothing:** another app holds it. Pick another in the seeded `settings.json`.
- **The window opened from a script doesn't really have the foreground** (Windows refuses), so focus hand-back rows (X10, O12, S7) can look broken. Click the subject window with computer use first so it truly has the focus, then test.
- **Second monitor rows** (O19, O20): check `--probe displays` first; with one display, report them as not run.

## Report

End with a table: section, rows passed, failed, not run. Then each failure as `ID: what was expected, what happened`, with the screenshot or file content that shows it, and the budget numbers next to their targets.
