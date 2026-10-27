# Media Mode — Update Plan

Builds on `MEDIA.md` and `CODEBASE_SUMMARY.md` §4.5 and §7.5. Media lives in `renderer/media/` (UI), `lib/media/` (main-process modules) and `media-preload.js`. Settings that the main process must know before the window exists go in `settings.json`; visual-only prefs go in `localStorage` (same split as the main window, §4.3).

---

## 1. Settings screen (replaces "Players and emulators" and "Controller" buttons)

**Goal:** one gear button in the top bar opens a Settings screen with sections, so the top bar only holds navigation and the `+` button.

**Current:** `renderer/media/index.html` has `#profilesBtn` ("Players and emulators") and `#controllerBtn` in the top bar; `renderer/media/media.js` has a `profilesScreen` section (~line 485) and a controller screen.

**Plan**
1. Replace `#profilesBtn` and `#controllerBtn` with one `#settingsBtn` (gear icon, `aria-label="Settings"`).
2. Add a `#settingsScreen` with a left list of sections, same pattern as the main window's settings (summary §7.2 item 11):
   - **Players and emulators** — the existing profile editor, unchanged in behaviour (`media:profiles`, `media:saveProfiles`).
   - **Colors** — see §3.
   - **Controller** — the existing pad mapping (`media:controller`, `media:saveController`).
   - **Library** (new, optional): default recursive scan, default library per folder type.
3. The screen keeps the existing screen-switch code (`state.screen`), so the controller navigation, Back (B) and focus handling already work on it.
4. Remove the "Players and emulators" wording from the library tile hint (`media.js` ~line 458, `Set up in Players and emulators`) and point it at Settings.

**Done when:** the top bar has `+`, tabs, gear and Exit; the gear reaches every settings section with the controller only (A, B, D-pad, LB/RB between sections).

---

## 2. "Add folder" becomes a `+` button, with a type choice

**Goal:** a plus button in the top bar. Choosing it asks what is being added first: a single program, or a folder.

**Current:** `#addFolderBtn` (text button) → `openAdd()` → a table of scan results for one folder (`media:scanPreview`, `media:addLibrary`).

**Plan**
1. Change `#addFolderBtn` to a `+` icon button (`aria-label="Add"`) in the top bar, and the empty-state button `#emptyAddBtn` to the same wording.
2. On click, open a small chooser (two large options, usable with the pad):
   - **Single app** — pick one `.exe` (e.g. a game launcher, a player). This creates a library entry of type `path` (the `type` values already exist in `library.js`: `profile`, `path`, `url`, `script`, `command`). Use the existing `media:pickProgram` dialog. Then ask for name, library (games/videos), platform and profile.
   - **Folder** — the existing flow: pick a folder, scan, preview, add. Used for movies and ROM libraries.
3. For **Folder**, make the platform choice explicit in the preview, since ROM folders may hold several platforms (the scanner already detects platforms; surface the detected one per group and let the user change it).
4. Both paths go through the existing `media:addLibrary` / a new `media:addApp` channel. The new channel is a thin wrapper that validates the path (`.exe` only, absolute, existing) like `libraryRequest` does in `main.js`.
5. Tests: `test/media-library.test.js` (or the existing `library.test.js`) for the single-app entry shape and its validation.

**Done when:** `+` opens the chooser; a single exe becomes a game tile; a folder runs the scan preview as before.

---

## 3. Customisable UI colours in Media settings

**Goal:** every colour in the Media UI can be changed from the Colors section, with a reset per colour and a reset-all.

**Current:** `renderer/media/media.css` defines tokens on `:root` (dark, and light under `prefers-color-scheme`): `--text`, `--muted`, `--accent`, `--on-accent`, `--panel`, `--raised`, `--warn`, `--danger`, and the background. The CSP blocks inline scripts, so the pre-paint script must be an external file.

**Plan**
1. Audit `media.css` for every hard-coded colour (not just tokens). Move any that are not tokens onto a token, so every colour the user can see is one token.
2. Define the editable list in one place (a JS module, e.g. `renderer/media/theme.js`): token name, label, default for dark and light.
3. Store the overrides in `localStorage` under `launchpad-media-colors` (a JSON map of token → colour), read by a small external script before first paint, which sets the properties on `:root`. This matches the existing per-viewer visual prefs (summary §4.3). Wrap every read and write in try/catch, as the Media window can run with storage blocked.
4. Colors section UI: one row per token, a colour input, a "Reset" link per row, and "Reset all colours" at the bottom. Live preview: change applies at once.
5. The main window's settings and the overlays share `launchpad-accent`; keep Media's accent separate so Media can be themed independently (note this in the Colors section).
6. Contrast: warn (not block) when text colour against a panel background is below WCAG AA 4.5:1.

**Done when:** every visible colour changes from the Colors section, the choice survives a restart, and Reset restores the defaults.

---

## 4. Launcher presets: add Dolphin, change RetroArch to GBA

**Current:** `lib/media/players.js` `PRESETS` has `mpv`, `vlc`, and `retroarch-snes` (Snes9x core, platform `snes`).

**Plan**
1. Add **Dolphin**: `kind: 'emulator'`, `platforms: ['gamecube', 'wii']`, args `['-b', '-e', '{file}']` (batch mode, exec the file, exit when it closes; check the Dolphin version in use, since `-b` and `-e` are the documented flags). Candidates: `C:\Program Files\Dolphin\Dolphin.exe`, `C:\Program Files\Dolphin-x64\Dolphin.exe`. Verify the platform ids against `lib/media/scan.js` before writing them.
2. Add **RetroArch (GBA)** as a new preset alongside the existing **RetroArch (Snes9x)** one: `id: 'retroarch-gba'`, platform `gba`, core `cores\\mgba_libretro.dll` (mGBA). Keep the SNES preset as is. Both share the RetroArch candidate paths.
3. Existing profiles keep working: a saved profile is a copy (`fromPreset` strips `candidates`), so an installed RetroArch/Snes9x profile is not changed. Mention this in the release note so the user knows to add a GBA profile.
4. Tests: `test/players.test.js` — every preset validates with `validateProfile`, and `fromPreset` returns a profile with no `candidates`.

**Done when:** the Players and emulators preset list shows Dolphin and RetroArch (GBA); an installed Dolphin is detected and can launch a GameCube file in batch mode.

---

## 5. Controller bumpers switch tabs

**Current:** `renderer/media/pad.js` maps button 4 (LB) → `tabPrev` and 5 (RB) → `tabNext`. In `media.js` (~line 958) those actions only run when `state.screen === 'home'`, so on any other screen the bumpers do nothing. The prompt bar (`prompts.js`) shows `LB / RB` for tabs.

**Plan**
1. Find out which screens have tabs. Home has Games / Videos / All. If other screens (Settings sections, Players list) should also switch between sections, route `tabPrev/tabNext` there too; otherwise make the prompt bar hide the `tabs` hint off Home so it doesn't promise something that does nothing.
2. Check `media.js` ~line 919 (`switchTab`) and the `.tab` click path: a pad switch must update `state.tab`, `aria-pressed` and the tile list together, the same as a mouse click, and reset focus to the first tile.
3. Wrap at the ends (decided). Use the same rule for the Settings sections.
4. Check the remap: the Controller section lets the user rebind `tabPrev` and `tabNext`. If the user has rebound them, the prompt bar reads the new names (`nameOf`, `media.js` ~line 792) — keep that.

**Done when:** with a pad on Home, LB and RB move Games → Videos → All and back; the focus lands on the first tile; the prompt bar shows the same keys.

---

## Order of work

| # | Item | Size | Risk |
|---|---|---|---|
| 5 | Bumpers switch tabs | small | low |
| 4 | Dolphin and GBA presets | small | low (verify ids and flags) |
| 2 | `+` button and single-app / folder chooser | medium | low |
| 1 | Settings screen (merges Players, Controller) | medium | medium (moves existing screens) |
| 3 | Colour customisation | medium | low (needs the colour audit first) |

Do §1 before §3, since the Colors section lives inside it.

## Testing

- Unit: `npm test`. New cases for §2 (single-app entry), §4 (presets validate, no `candidates` in saved profiles).
- Manual with a pad: §5 and §1 (every settings section reachable without the mouse).
- Manual: §3 colour changes survive restart, and Reset works per colour and for all.

## Open questions for you

Decided: §4 keeps the SNES (Snes9x) preset alongside the new GBA one.

Decided: §1 moves Controller into Settings (no top-bar button). §5 wraps at the ends of the tabs.

No open questions remain in this plan.
