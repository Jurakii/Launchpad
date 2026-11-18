# Launchpad 1.3: what's left

Written 2026-11-18, at the 1.3.0 release. Plan and history: `RUST_PORT_PLAN.md`.
The flow from here: you test 1.3.0 → bugs go under "Bugs found in 1.3.0" → we fix them and the rest of this list → release 1.3.1, which is also the test of the in-app updater.

## 1. Your tests on 1.3.0

Back up `%APPDATA%\launchpad` first (copy the folder somewhere). Each item is a row of `.claude/skills/test-launchpad/features.md`; these are the ones a test can't drive without your real keyboard, mouse, controller or install.

- [ ] **Install over 1.2.2.** Run `Launchpad-Setup-1.3.0.exe` (UAC prompt). It should close 1.2.2, replace it in `C:\Program Files\Launchpad`, and start 1.3. Then check:
  - [ ] your apps, islands, layouts, scenes, Media library and settings are all there
  - [ ] the look carried over (theme, accent, wallpaper, bar layout, tray order)
  - [ ] "Start with Windows" is still on if it was (Settings → General), and Launchpad starts after a sign-out/in
  - [ ] the hotkey works
  - [ ] Apps & Features shows one Launchpad, version 1.3.0
- [ ] **X2** the hotkey shows the window from another app and hides it when it has the focus
- [ ] **X5** left-clicking the tray icon toggles the window; the tray menu works
- [ ] **X10** hiding Launchpad gives the keyboard back to the app you were in
- [ ] **F1–F5** drag files, folders, shortcuts and programs in from Explorer (Alt while dropping adds a link)
- [ ] **Organizer:** hover a panel zone (O7); auto-hide reveal at the top edge (O8); a real fullscreen game or video hides the panel (O9); drag a window to the top and drop it on a zone (O10); the grid hotkey and rebinding it (O12, O13); the scenes menu closes on an outside click (S7); rearrange or unplug a display (O19, O20); an elevated (admin) window (O21); restarting Explorer (O22)
- [ ] **Media:** the hotkey toggles Media (M1); art from a dropped file, a real web address and a too-large image (M9); with a controller: move, A, B, LB/RB, and remap a button in Settings → Controller (M11, M12). Controller maps reset to defaults once (1.2's were keyed by Chrome's pad ids); the dead zone carries over.
- [ ] **Uninstall** (only if you want to try it; reinstall afterwards): leaves `%APPDATA%\launchpad` and nothing else
- [ ] Anything else you use day to day that looks or behaves differently from 1.2.2

## 2. Bugs found in 1.3.0

(Add them here, or tell me and I will.)

## 3. Before or with 1.3.1

- [ ] Fix the bugs above.
- [ ] **Exe icon:** the icon ships as `build\icon.ico` beside the exe, so Explorer and Task Manager show a generic icon for `Launchpad.exe`. Embed it as a resource and have the tray read it from there.
- [ ] **Release 1.3.1 and test the updater:** 1.3.0 → Settings → Backup & updates finds 1.3.1 → Download & Install → UAC prompt → Launchpad comes back as 1.3.1. Also the startup notification with "Notify me when an update is available" on.
- [ ] Check that 1.2.2 doesn't offer 1.3 (its Check for updates shows an error, which its Settings note explains). Only testable on a PC still on 1.2.2.

## 4. Phase 7: cut-over (after 1.3.1 is settled)

- [ ] Full parity pass: every row of `features.md` on 1.2.2 and 1.3 side by side; fix or write down each difference.
- [ ] Budgets in `RUST_PORT_PLAN.md` §2 not measured yet: cold start to window (≤ 150 ms), hotkey to visible (≤ 50 ms), tray widget actions (≤ 20 ms), main window with 200 items and a wallpaper, Media open with art (≤ 80 MB).
- [ ] 24-hour soak with the organizer on: memory stays flat; no panel space left behind after quit, kill or crash.
- [ ] Delete the Electron tree (`main.js`, `lib/`, `renderer/`, preloads, `scripts/`, `package*.json`, `test/`) and the npm job in `.github/workflows/test.yml`.
- [ ] Rewrite `CODEBASE_SUMMARY.md` and the README's build section for the Rust build.

## 5. Decisions still open

- [ ] **Licence:** Slint's royalty-free licence (the "Made with Slint" line in Settings → About is what it asks for, and it is there), or make the repo GPLv3.
- [ ] **Install scope:** the installer is per-machine only (Program Files, UAC on every install and update), matching how 1.2.2 is installed here. Keep that, or offer per-user too?

## 6. Known limits, kept on purpose (each has a `shortcut:` note in the code)

Upgrade one only if it shows up in use.

- Dates in the grid are always US format (`grid_logic.rs`); 1.2 followed the system locale.
- Name sorting uses simple lower-cased order in a few places instead of full locale collation (`store.rs`, `modals.rs`, `scan.rs`).
- Wi-Fi uses the first adapter only (`wifi.rs`).
- Unavailable-network-path checks run on the UI thread at refresh (`grid.rs`), as in 1.2; a dead network share can stall it.
- Backup import copies stored files on the UI thread (`settings.rs`).
- The exe is 12.48 MiB against an original 12 MiB goal; the gate is 13 MiB. Getting under 12 would mean patching Slint's text stack (see `RUST_PORT_PLAN.md`, Phase 6).
