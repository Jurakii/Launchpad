# Launchpad — Codebase Summary

A personal Windows desktop-replacement launcher (Electron 44). It runs as a full-screen-capable window with a custom taskbar, an app/folder grid, and a set of system-tray widgets (volume, USB, Bluetooth, Wi-Fi, clock, power). It is meant for apps that don't behave well with the native taskbar or Start Menu.

- **Version:** 1.0.4 (`package.json`)
- **Repo:** `Jurakii/Launchpad` on GitHub, branch `main`
- **Platform:** Windows only (PowerShell helpers, `Clear-RecycleBin`, `shutdown.exe`, `ms-settings:`, etc.)
- **Size:** ~1,300 lines in `main.js`, ~3,400 in `renderer/renderer.js`, ~1,550 in `renderer/styles.css`. User-facing docs are in `README.md`.

> Keep this file current. When you change architecture, IPC channels, storage keys or the release process, update the matching section below.

---

## 1. Layout

```
main.js              Electron main process: window, tray, hotkey, auto-updater, ALL ipcMain handlers
preload.js           contextBridge → window.launcherAPI (the only renderer↔main surface)
overlay-preload.js   contextBridge → window.overlayAPI for the drag overlay (listen only; listed in build.files)
panel-preload.js     contextBridge → window.panelAPI for the organizer panel (listed in build.files)
grid-preload.js      contextBridge → window.gridAPI for the grid placer (listed in build.files)
scenes-menu-preload.js  contextBridge → window.scenesMenuAPI for the panel's scenes menu (listed in build.files)
media-preload.js     contextBridge → window.mediaAPI for the Media Mode window (listed in build.files), see §7.5
lib/                 Main-process modules (store, layout, launcher, icons, favicon, settings, safe-file, window organizer, + system wrappers)
lib/media/           Media Mode main side: library.js (library.json store), players.js (profiles.json, argv building), scan.js (folder scan, platform detection, merge), launch.js (tracked launches), art.js + art-protocol.js (artwork and the lpmedia: scheme), controller.js (pad settings), backup.js (its part of backups)
lib/*.ps1            PowerShell helpers invoked by lib/*.js (asarUnpacked, see §6)
spikes/organizer/    Throwaway Win32 experiments for the window organizer (not shipped, not tested; README has the findings)
renderer/            index.html, renderer.js (all UI logic), styles.css, icons/*.svg; overlay.html/.js/.css (drag overlay), panel.html/.js/.css (organizer panel), grid.html/.js/.css (grid placer), scenes-menu.html/.js/.css (the panel's scenes menu) and cells.js (cell math shared by the grid and the Layouts editor), see §5.1; media/ (Media Mode window), see §7.5
scripts/build.js     Wraps electron-builder; re-embeds icon; renames installer to match latest.yml
scripts/fix-exe-icon.js  Patches the exe icon post-build (safety net)
build.bat            Local build helper for Windows (npm install → npm run dist)
.github/workflows/release.yml  Manual "Release" workflow (windows-latest) → GitHub release
.github/workflows/test.yml     "Test" workflow: npm test on push to main and on PRs (windows-latest)
build/icon.ico       App/window/tray icon (electron-builder win icon)
test/                node:test unit tests (store, settings, favicon, layout, zones, layouts, strip, autohide, cells, scenes, and Media Mode's library, players, scan, launch, pad, art, controller, media-backup); helpers.js stubs `electron` with a temp userData
Launchpad/           Icon sources (not used at runtime)
```

Scripts: `npm start` (`electron .`), `npm run dist` (`node scripts/build.js`), `npm test` (`node --test test/*.test.js`).
Dependencies: `electron-updater` (runtime); `electron`, `electron-builder` (dev).

---

## 2. Process model and security

- `BrowserWindow` uses `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`, and `preload.js`.
- The renderer never touches Node or the filesystem. It calls `window.launcherAPI.*`, which `preload.js` maps 1:1 to `ipcRenderer.invoke('<channel>')`.
- Single-instance lock (`app.requestSingleInstanceLock`). A second launch just calls `showWindow()` on the first instance.
- The CSP blocks inline scripts. That's why the early theme/accent-restoring code lives in `renderer.js` (top of file), not in `index.html`.

---

## 3. Main process (`main.js`)

### 3.1 Lifecycle
- `app.whenReady`:
  1. Seeds default apps (File Explorer, Recycle Bin) if `apps.json` doesn't exist yet.
  2. `createWindow()`, `createTray()`.
  3. `registerHotkey(settings.hotkey || 'CommandOrControl+Space')`.
  4. `applyHotkeyAppFocusSetting(settings.disableHotkeyInApps)`.
  5. `panel.releaseLeftovers()` (gives back the space of panels a killed run left docked, see §6.2), then `organizer.configure(settings.organizerOptions())` (panels, drag snapping; starts the window-manager helper only when the organizer is on).
  6. If packaged and `notifyUpdates` is on, a background `autoUpdater.checkForUpdates()`.
- Closing the window hides it to the tray by default (`minimizeToTrayOnClose !== false`). `isQuitting` distinguishes a real quit. With that setting off, the window's `closed` event calls `app.quit()`: the organizer's panel, overlay and grid windows would otherwise keep the app running.
- `will-quit` empties `appbars.json` (`panel.forgetDocked()`), stops the window-manager helper (which gives back the panels' AppBar space as it exits) and calls `globalShortcut.unregisterAll()`.
- `activate` (macOS-style re-open) shows or recreates the window.

### 3.2 Window, tray, hotkey
- `createWindow()`: 960×680, min 640×420, menu bar hidden. Honours `startFullscreen`. Pushes `fullscreen-changed` events to the renderer.
- `createTray()`: tray icon. Left-click toggles the window. Context menu: Show / Start with Windows (checkbox) / Quit.
- `toggleWindow()`: hide if visible and focused, otherwise show and focus.
- **Global hotkey:** `registerHotkey(accel)` swaps the old accelerator for the new one. If the new one is claimed by another app it keeps the old one and returns false. While the Media Mode window is open, `toggleWindow()` shows or hides that window instead of the main one, so the hotkey brings Media Mode back from a game.
- **"Disable hotkey while another app is focused":** keeps the window-manager helper running (`windowmgr.setKeepAlive('hotkey')`) and follows its `foreground` events: `setHotkeySuspended(kind === 'app')` unregisters or re-registers the hotkey. Launchpad's own windows (`own`), the desktop and the taskbar/Start/search shell (`shell`) don't count, so the hotkey can still show and hide Launchpad from them. `helperExit` un-suspends it until the helper is back. (This replaced `lib/foreground.ps1`, which polled every 200 ms.)
- **Auto-launch:** `app.setLoginItemSettings`. In dev it points at `electron.exe` + project path (`loginItemOptions`).

### 3.3 Auto-update
- `electron-updater`, `autoDownload = false`, `autoInstallOnAppQuit = false`.
- Events → `update:status` push to the renderer: `available` / `downloading` / `downloaded` / `none` / `error`.
- `update:check` and `update:install` (first call downloads, second call `quitAndInstall`). Both return `unsupported` / error when not packaged.
- Feed = GitHub provider (`Jurakii/Launchpad`), via `latest.yml` + `.blockmap` + installer. See §8.

### 3.4 IPC channels (main side)

Renderer calls these through `launcherAPI` (preload method → channel):

| Area | Channels |
|---|---|
| Apps/items | `apps:list`, `apps:loadIssue`, `apps:add`, `apps:update`, `apps:remove`, `apps:launch`, `apps:detectMissingIcons`, `apps:restoreDefaults` |
| Folders ("islands") | `folder:add`, `item:rename`, `item:setColor`, `item:move`, `item:setPosition`, `item:setPositions` (batch), `item:setCollapsed`, `item:setShowNames`, `item:setSize` |
| Taskbar pins | `item:togglePin`, `item:setPinned`, `item:setPinOrder` |
| Dialogs / icons | `dialog:pickTarget`, `dialog:pickIcon`, `icon:extractPreview`, `icon:fetchFaviconPreview` |
| Volume / audio | `system:getVolume`, `system:setVolume`, `system:setMuted`, `system:listAudioDevices`, `system:setAudioDevice`, `system:listAudioSessions`, `system:setSessionVolume`, `system:setSessionMuted` |
| USB | `system:listUsbDrives`, `system:ejectDrive` |
| Open windows | `system:listOpenWindows`, `system:focusWindow` |
| Window organizer | `organizer:listLayouts` (the layouts that aren't hidden, for "Snap to"), `organizer:placeWindow` (handle, layoutId, zoneId → `{ ok, error }`); push `overlay:state` to the drag overlay windows (not the main window) |
| Layouts editor | `organizer:listAllLayouts`, `organizer:saveLayout` (layout), `organizer:removeLayout` (id), `organizer:setLayoutHidden` ({ id, hidden }), `organizer:setGap` (gap). Each returns the library `{ layouts: [{ ...layout, builtIn, hidden }], gap }` and throws a ready-to-show message for an invalid change (`lib/layouts.js`). Changes call `organizer.layoutsChanged()` so the panels follow. |
| Organizer panel (`panel-preload.js`, not the main window) | `panel:snap` (layoutId, zoneId → `{ ok, error }`; moves the focused window into that zone on the panel's display), `panel:hover` (`{ layoutId, zoneId }` or null; full-size preview on the overlay), `panel:openGrid` (→ `{ ok, error }`; opens the grid placer on the panel's display, or closes it if it's open there), `panel:launchpad` (`{ action, id, ids, x, y }` → `{ ok }`; the top bar's breadcrumb and buttons, only from the panel carrying them: `fullscreen` and `mediaMode` are done in main, the rest is forwarded to the main window as `topbar:action` after `showWindow()`); push `panel:state` (`{ layouts, focused: { title, iconDataUrl, snappable } \| null, helperUp, gridHotkey, aspect, height, launchpad: { crumbs, fullscreen } \| null }`). Main maps the sender to its panel's display. |
| Top bar (main window ↔ panel) | `topbar:setCrumbs` (`[{ id, name }]`, Home first; sent by `renderBreadcrumb` when it changes); push `topbar:state` (`{ hosted, inset }`: a panel carries the toolbar, so the window hides its own; px to start fullscreen content below a docked panel) and `topbar:action` (`{ action, id, ids, x, y }` from the panel: `crumb`, `crumbMenu`, `drop`, `layouts`, `settings`, `newIsland`, `addApp`; ignored while a dialog is open). |
| Scenes (main window) | `organizer:listScenes`, `organizer:saveScene` (optional id: save over that scene, an island's again holding only its apps), `organizer:renameScene` ({ id, name }), `organizer:removeScene` (id), each → the scene list; `organizer:applyScene` (id → a message: "Placed 3 windows. Not open: Spotify."), `organizer:saveIslandScene` (islandId → `{ scenes, message }`), `organizer:openIslandScene` (islandId → message); push `scenes:changed` (a panel's menu saved one) and `scenes:open` (its "Manage scenes…"). Errors are thrown as ready-to-show messages. |
| Scenes menu (`scenes-menu-preload.js`; and the panel's `panel:openScenes` with the button's screen rect, push `panel:message` for the result) | `scenesMenu:resize` (its height, after each render; it's shown on the first), `scenesMenu:apply` (id), `scenesMenu:save`, `scenesMenu:manage`, `scenesMenu:close`; push `scenesMenu:state` (`{ scenes: [{ id, name, count, island }] }`). Picks close the menu first (`organizer.takeScenesMenu`) and report on the panel. |
| Grid placer (`grid-preload.js`, not the main window) | `grid:place` (`{ x, y, w, h }` in cells → `{ ok, error }`; the grid closes on success), `grid:cancel`; push `grid:state` (`{ cols, rows, gap, area, frame, title, iconDataUrl }` or null). Only the open grid's sender is accepted. |
| Media Mode entry (main window, `launcherAPI.openMediaMode`) | `media:open` (opens the Media Mode window, or brings it forward) |
| Media Mode (`media-preload.js`; every channel registered with `mediaHandle()`, which refuses any sender but the Media Mode window) | `media:library` (→ `{ entries, loadIssue }`; each entry carries `art: { slot: updatedAt }` for the slots whose file exists), `media:platforms` (→ `[{ id, name }]`), `media:profiles` (→ `{ profiles, defaults, presets, detected }`), `media:saveProfiles` (`{ profiles, defaults }` → `{ ok, data }`, or `{ ok: false, id, errors }` for the first invalid profile), `media:pickProgram` / `media:pickFolder` (dialogs → path or null), `media:scanPreview` (`{ root, library, recursive, platforms }` → `{ count, groups }`), `media:addLibrary` (same request → `{ added, missing, restored }`; also saves the folder to `settings.libraries`), `media:launch` (id → `{ tracked, pid }`, `{ tracked: false }` or `{ alreadyRunning: true }`), `media:running`, `media:stop` (id), `media:close` (closes the window and shows the main one), `media:setArtFile` (id, slot → opens a picker; the updated entry, or null if cancelled), `media:setArtPath` (id, slot, path of a file dropped on a slot, from `webUtils.getPathForFile` in the preload), `media:setArtUrl` (id, slot, http(s) address), `media:clearArt` (id, slot), each → the updated entry; `media:controller` / `media:saveController` (`{ deadZone, pads }`); push `media:exited` (`{ id, code, seconds, handoff, stopped }`, to the window that launched it, which is brought forward unless `handoff`). Folder requests are checked in main (`libraryRequest`: existing absolute folder, known library, known platform ids). |
| Bluetooth | `system:listBluetoothDevices`, `system:disconnectBluetoothDevice` |
| Power | `system:shutdown`, `system:restart`, `system:sleep` |
| Wi-Fi | `system:listWifiNetworks`, `system:connectWifi`, `system:disconnectWifi`, `system:openNetworkSettings` |
| Misc system | `system:emptyRecycleBin` |
| Window | `window:isFullscreen`, `window:toggleFullscreen` (+ push `fullscreen-changed`) |
| Settings | `settings:getAutoLaunch/setAutoLaunch`, `getStartFullscreen/setStartFullscreen`, `getHotkey/setHotkey`, `getDisableHotkeyInApps/setDisableHotkeyInApps`, `getNotifyUpdates/setNotifyUpdates`, `getGoogleFaviconFallback/setGoogleFaviconFallback`, `getMinimizeToTrayOnClose/setMinimizeToTrayOnClose`, `getOrganizerEnabled/setOrganizerEnabled`, `getOrganizerPanel/setOrganizerPanel`, `getOrganizerSnapOnDrag/setOrganizerSnapOnDrag` (the organizer setters call `organizer.configure`), `getOrganizerHotkey/setOrganizerHotkey` (the grid placer; refuses the show/hide hotkey, and `setHotkey` refuses the grid's) |
| Updates | `app:getVersion`, `update:check`, `update:install` (+ push `update:status`) |
| Backup | `backup:export`, `backup:import` |

Pattern used throughout: `store.load()` → mutate → `store.save(apps)` → `return decorate(apps)`. `decorate()` adds `iconDataUrl` and a `missing` flag (target path doesn't exist) to every entry before it reaches the renderer.

### 3.5 Non-obvious behaviour
- **Removing an island** moves its children up to the island's parent and deletes their `gridX/gridY` so they re-place.
- **Moving an item** to a new parent clears its grid position (each parent is its own coordinate space).
- **Recycle Bin** is a `type: 'command'` entry with `special: 'recycleBin'`. Its icon is drawn by the renderer, not extracted.
- **Shortcut handling:** `.lnk` targets are resolved through `shell.readShortcutLink` into the real target/args/cwd on add/update.
- **Icons:**
  - Auto-fetched from the target file via `app.getFileIcon` (for `path`/`script` entries).
  - Favicons for URL items (`lib/icons.js` `fetchFavicon`, via Electron `net.fetch`):
    1. The page's best `<link rel="icon">` or apple-touch-icon (largest declared size first, SVG skipped).
    2. Then `/favicon.ico`.
    3. Then Google's `s2/favicons` service, only if `googleFaviconFallback` is on.
    - `.ico` files are decoded by `lib/favicon.js` `decodeIco`, because `nativeImage` can't read ICO from a buffer.
    - Images larger than 128px are resized down. Non-http(s) targets (`steam://` etc.) get no favicon.
  - Custom icons are resized to 128×128 PNG.
  - Saved as `<userData>/icons/<id>.png`.
  - `iconIsCustom` protects a user-chosen icon from being overwritten by auto-detect.
- **Open-window icons** (taskbar window list, organizer panel) are cached per exe path by `icons.exeIconDataUrl`.
- **Backup format:** `{ version: 3, apps, layouts, library, profiles }`. `library` and `profiles` are Media Mode's (`lib/media/backup.js`), without art; importing them replaces the library (ids kept, so art folders still match) and the profiles, and a failure there comes back as `mediaError`. Version 2 has neither and leaves them alone. `apps` is the entries, each icon embedded as base64 `iconData` (stripped of `iconPath`); on import, icons are rewritten to `<userData>/icons/<id>.png`. `layouts` is what `layouts.json` holds (`layouts.exportData()`: layouts, hidden list, gap and scenes) and replaces it on import (`importData`, invalid layouts and scenes dropped); a failure there is reported as `layoutsError` without undoing the apps. Older backups are a bare array of entries: they still import and leave the layouts alone.

---

## 4. Data model and storage

Stored under Electron's `app.getPath('userData')`:

| File | Owner | Contents |
|---|---|---|
| `apps.json` | `lib/store.js` | JSON array of entries (apps, islands, pins, positions) |
| `apps.json.bak` | `lib/store.js` | The previous `apps.json`, copied before every save (only if it still parses) |
| `apps.corrupt-<ts>.json`, `settings.corrupt-<ts>.json`, `layouts.corrupt-<ts>.json` | store / settings / layouts | Unreadable files kept aside on load (see §4.1) |
| `settings.json` | `lib/settings.js` | Main-process prefs (see below) |
| `layouts.json` | `lib/layouts.js` | User window layouts, which layouts are hidden, the organizer gap and the scenes (see §4.4). Written by the Layouts editor and scenes; optional |
| `appbars.json` | `lib/panel.js` | Window handles of the panels docked right now; emptied on quit, released at the next start if not (§6.2) |
| `icons/<id>.png` | `lib/icons.js` | Per-item icon |
| `library.json`, `library.json.bak`, `library.corrupt-<ts>.json` | `lib/media/library.js` | Media Mode's games and videos (§4.5). Same atomic write, backup, restore and read-only behaviour as `apps.json` |
| `media/<id>/<slot>.jpg` or `.png` | `lib/media/art.js` | Media Mode art per library entry: `cover`, `banner`, `screenshot` as JPEG, `logo` as PNG, shrunk to fit 600×900, 1920×1080, 1280×720 and 800×400 |
| `profiles.json`, `profiles.corrupt-<ts>.json` | `lib/media/players.js` | Media Mode's players and emulators and the default per platform (§4.5). An unreadable one is moved aside, like `settings.json` |

### 4.1 Entry shape (`apps.json`)
Common: `id` (UUID), `kind` (`'app'` | `'island'`), `parentId` (null = Home, or an island id), `pinned`, `pinOrder`, `createdAt`, `gridX`, `gridY`, `color`.

- **App** (`kind: 'app'`): `name`, `type` (`path` | `url` | `script` | `command`), `target`, `args`, `cwd`, `interpreter`, `command`, `iconPath`, `iconIsCustom`, `lastLaunched`, `launchCount`, optional `special: 'recycleBin'`.
- **Island** (`kind: 'island'`): `collapsed` (default `true`), `w`/`h` (cells, used when expanded), `showNames`. Formerly `'folder'`; `normalize()` migrates it.

`lib/store.js` `normalize()` backfills defaults for old entries. `assignMissingPositions()` gives every entry a grid cell, grouped per parent. `assignMissingPinOrders()` gives pinned items a stable order.

**Corruption handling.** All writes go through `lib/safe-file.js` `writeFileAtomic`: write `<file>.tmp`, fsync, rename over the file. A crash can't leave a half-written file behind.
- `store.load()` tells three cases apart:
  - **Missing** (`ENOENT`): empty list, first run.
  - **Locked / permission error:** throws, so the IPC call fails and nothing is saved.
  - **Unparseable** (bad JSON, or not an array): copies the file to `apps.corrupt-<ts>.json`, then either
    - restores `apps.json.bak` (`loadIssue.state = 'restored'`), or
    - goes **read-only** if there's no usable backup (`'readOnly'`). `save()` then throws, and the next `load()` that reads a valid or missing `apps.json` clears read-only.
- `store.replaceAll()` (backup import) is allowed in read-only mode.
- `apps:loadIssue` → `store.getLoadIssue()`. The renderer shows it once after the first `listApps()`.
- `settings.json` that won't parse is *moved* to `settings.corrupt-<ts>.json`, and defaults are used. `settings.save()` throws instead of merging into `{}` when the file can't be read.
- **Backup import** gives a fresh UUID to any entry whose `id` isn't a UUID or is a duplicate (ids become icon filenames). Children are re-pointed to the new id. A `parentId` that isn't an island in the backup becomes `null`.

### 4.2 `settings.json` keys
`hotkey` (default `CommandOrControl+Space`), `startFullscreen`, `minimizeToTrayOnClose` (default true), `notifyUpdates`, `disableHotkeyInApps`, `googleFaviconFallback` (default true; absent = on), `organizerEnabled` (window organizer master switch; default false), `organizerPanel` (`always` / `autoHide` / `off`; default `always`), `organizerSnapOnDrag` (default true), `organizerHotkey` (grid placer; default `CommandOrControl+Alt+G`, registered only while the organizer is on). `settings.organizerOptions()` fills in the organizer defaults (the hotkey comes back as `gridHotkey`). `controller`: Media Mode's pad settings, `{ deadZone, pads: { <Gamepad id>: { <button index>: action } } }`; `lib/media/controller.js` clamps the dead zone to 0.1–0.9 (default 0.35) and drops invalid buttons and actions, and any pad map without `select` and `back`. `libraries`: Media Mode's library folders, `[{ root, library, recursive, platforms }]` (one per folder and library; re-adding replaces it).

### 4.3 Renderer `localStorage` keys (UI-only prefs, read before first paint)
`launchpad-theme` (`light`/`dark`), `launchpad-accent`, `launchpad-wallpaper`, `launchpad-taskbar-style`, `launchpad-topbar-style`, `launchpad-taskbar-edge`, `launchpad-taskbar-position` (`right` = pinned side), `launchpad-taskbar-align` (`center`), `launchpad-tray-align` (`center`/`shared`), `launchpad-tray-order` (JSON), `launchpad-hide-add-buttons` (`1`), `launchpad-hide-app-names` (`1`), `launchpad-topbar-hosted` (`1` while the panel carries the toolbar, so the window starts without it instead of jumping once main confirms). The old `launchpad-panel-style` is removed at startup: the panel follows `launchpad-topbar-style` now.

The overlay and panel windows load from `file://` like the main window, so they share this `localStorage`: they read `launchpad-theme`, `launchpad-accent` (and the panel `launchpad-topbar-style` and `launchpad-hide-add-buttons`) and restyle on the `storage` event when Settings changes them.

Split of responsibility: anything the main process must know before a renderer exists lives in `settings.json`. Pure visual prefs live in `localStorage`.

### 4.4 `layouts.json` (window organizer)
`{ version: 1, gap, hidden, layouts: [...], scenes: [...] }`. `gap` is a whole number of DIP from 0 to 64 (default 8). `hidden` lists layout ids left out of the panel, the drag strip and "Snap to" (`layouts.visible()`; if every layout is hidden, all are shown). A layout is `{ id, name, cols, rows, zones: [{ id, name, x, y, w, h }] }`, with zones in whole cells of its own grid (max 12×12, no overlaps; `zones.validateLayout`). User layouts get a UUID from the editor; names are trimmed to 40 characters.
- The five built-ins (`halves`, `thirds`, `focus`, `main-stack`, `quarters`) live in code and are never written to the file, so a later version can change them. They can be hidden, not changed or deleted. A file layout reusing a built-in id, or an id already used, is ignored; `hidden` ids that aren't layouts are dropped.
- `load()` returns built-ins first, then the user's valid layouts; invalid ones are logged and skipped. Unknown top-level keys survive a `save()`.
- The editor's changes (`saveLayout`, `removeLayout`, `setHidden`, `setGap`) go through `update()`, which reads with `read()` rather than `load()`: a file that's locked right now makes the change throw instead of being saved over with only the built-ins.
- **Scenes** (`lib/scenes.js` checks them, `normalizeScenes` drops invalid ones on load): `{ id, name, islandId, savedAt, windows: [{ exe, title, display: { id, bounds }, rect: { x, y, w, h }, maximized }] }`. `rect` is fractions of that display's work area (0–1); `display` is matched again by id, then by bounds, else the primary. `islandId` is set for a scene saved from an island (one per island); one whose island is gone is just a scene. Changed with `saveScene` / `renameScene` / `removeScene` through `update()`, and carried by backups along with the layouts.
- Corruption handling is the same as `settings.json`: moved aside to `layouts.corrupt-<ts>.json`, defaults used.

### 4.5 Media Mode: `library.json` and `profiles.json`
Kept apart from `apps.json` on purpose: library entries would otherwise each get a Home grid cell, go out with every `apps:list`, and be embedded in backups. `MEDIA_MODE.md` has the full plan.

- **Library entry:** `id` (UUID; also the future `media/<id>/` art folder, so `fixIds()` replaces any non-UUID or duplicate id on load and `mediaDirFor()` refuses anything else), `library` (`games` | `videos`), `name`, `platform` (scan.js platform id or null), `type` (`profile` | `path` | `url` | `script` | `command`), `profile` (id or null = the platform's default), `target`, `available` (false when a rescan can't find the file; never removed), `launchCount`, `lastLaunched`, `playSeconds` (tracked sessions, hand-offs excluded), `createdAt`, `media` (slots `cover`, `banner`, `logo`, `screenshot`, `video`: `{ path, source, updatedAt }` or null; `path` is the file name inside `media/<id>/`, `source` is `user` or `scraper:<name>`, and scrapes must leave `user` slots alone; `video` isn't used yet), `meta` (`year` for videos so far).
- **Profiles file:** `{ profiles: [{ id, name, kind: 'player' | 'emulator', exe, args: [string], platforms: [id] }], defaults: { <platform id or 'videos'>: profileId } }`. `exe` must be a full path to an `.exe`: `.bat`/`.cmd` are refused because Node would run them through `cmd.exe`, which interprets `&` and `%` in file names. `args` is an array; `{file}`, `{dir}` and `{name}` are replaced inside each element with a function replacer (so `$&` in a name is literal), and the program runs with no shell from its own folder. `save()` drops invalid or duplicate profiles and defaults that point nowhere.

---

## 5. Layout engine (`lib/layout.js`)

- Freeform, snap-to-grid canvas. Cells are integers. Each parent (Home or an island) has its own coordinate space.
- Loose apps and collapsed islands are 1×1. Expanded islands are `w×h` (default 3×2; min 2×2, max 8×6).
- `findFirstFreeCell` (reading order, 8 columns) for new items. `findNearestFreeCell` spirals outward for drops.
- Every write path in `main.js` calls `findNearestFreeCell` against `siblingOccupiedFootprints(...)`, so overlaps are resolved on the main side.

### 5.1 Window organizer (`lib/zones.js`, `lib/strip.js`, `lib/organizer.js`, `lib/panel.js`, `lib/autohide.js`, `lib/grid.js`, `renderer/cells.js`)
Separate from the launcher grid above, but the same integer-cell idea: a layout divides a display's work area into `cols × rows` cells.
- `zones.zoneRect(layout, zone, workArea, gap)` → DIP rect. Grid lines are rounded once and shared by neighbouring zones; the gap insets the outer edges by `gap` and is split ceil/floor across inner lines, so neighbours are exactly `gap` apart.
- `organizer.snapWindow(handle, layoutId, zoneId)`:
  1. Asks the helper which monitor the window is on (for a minimized window, the one it restores to).
  2. Matches that monitor to an Electron display and computes the zone in DIP.
  3. Converts it with `screen.dipToScreenRect`, then `place` and `focus` via the helper.
  4. Maps helper failure codes to user-facing messages (`gone`, `denied` for elevated windows).
- **`organizer.configure({ enabled, panel, snapOnDrag, gridHotkey })`** applies the settings. `enabled` is the master switch: it keeps the helper running (`setKeepAlive('organizer')`) and registers the grid hotkey, and while it's off only the taskbar's "Snap to" menu works. When turning off, the panels are removed before the helper is let go, so it's still there to give their space back.
- **Hidden layouts:** the panel, the drag strip and "Snap to" offer `layouts.visible()`. `organizer.layoutsChanged()` reloads the panel's copy after an edit; the strip and the grid read `layouts.json` each time they open.
- **Drag to snap** (`organizerEnabled` and `organizerSnapOnDrag`):
  1. The helper's `dragStart` event starts a 10 ms poll of `screen.getCursorScreenPoint()`. The dragged window owns mouse capture, so nothing of ours gets mouse events during a drag. 16 ms would run at ~32 Hz, because Windows timers tick every 15.6 ms.
  2. `strip.shouldShow` shows the strip when the cursor is within 48 DIP of the work area's top, or over the strip itself (so a quick flick can't slip between polls). It stays while the cursor is within 48 DIP of the strip. `strip.zoneAt` finds the hovered zone.
  3. Main draws nothing itself. It sends the overlay for that display (a transparent, click-through, non-focusable, always-on-top window over its work area, created lazily per display and destroyed on any display change) the strip rects, the hovered zone and the full-size preview, all relative to the overlay. Messages go only when something changes.
  4. On `moveEnd`, if a zone is hovered and the drag wasn't cancelled, the window is placed in that zone on the cursor's display. Failures show a system notification, since Launchpad's own window is usually hidden.
- `strip.computeStrip` lays out one thumbnail per layout, centred at the top of the work area, in the display's aspect ratio. Thumbnails shrink to fit narrow displays. With the panel docked, the work area starts below it, so the strip hangs from the panel's bottom edge.
- **Panel** (`lib/panel.js`, `renderer/panel.*`): one 32 DIP bar per display. Left: every visible layout as a small thumbnail whose zones are buttons; clicking one moves the focused window there, on that display (`organizer.snapFromPanel`), and hovering previews the zone full size on the overlay. Thumbnails that don't fit are clipped. Centre: the focused window's icon and title. Right: the grid placer button (its tooltip names the hotkey), the scenes button, date and clock. Zones and the grid button are dimmed when the focused window can't be snapped. Errors show in the title spot for 3 s.
  - The window is `focusable: false` (`WS_EX_NOACTIVATE`), so clicking it leaves the user's window in the foreground; that's the window that moves. Topmost is set with `setAlwaysOnTop(true, 'screen-saver')` after creation: the constructor's `alwaysOnTop` doesn't take for these windows.
  - It's transparent (for the looks), and Windows won't make a transparent Electron window shorter than its minimum window height (39 px at 100%). `setPanelBounds` cuts it to the panel's height with `setShape` (a window region), so the extra strip is neither drawn nor clickable.
  - **Always shown:** the helper registers the window as an AppBar on the top edge (`appbarSet`), so the work area shrinks and maximized windows stop below it. Explorer's notifications reach the window through `hookWindowMessage` (`WM_APP + 0x42`): `ABN_POSCHANGED` re-docks it (debounced), `ABN_FULLSCREENAPP` triggers a fullscreen check. The `TaskbarCreated` broadcast (Explorer restarted and forgot its AppBars) re-registers it. A panel is only re-docked when its display's bounds or scaling change, not on the work-area change docking itself causes.
  - **Auto-hide:** nothing is reserved. A 50 ms cursor poll feeds `autohide.step` (pure, tested): the panel comes down after the cursor rests 250 ms against the display's top edge (so a quick flick to a maximized window's close button doesn't trigger it), and goes 500 ms after the cursor leaves it (with a 16 DIP margin). Before coming down it runs a fresh fullscreen check, since no AppBar notification reaches it. It stays up during window drags.
  - **Fullscreen:** a panel hides while its display is covered by a fullscreen window (helper `fullscreen` command, matched to displays by physical bounds). Launchpad's main window is passed as `own`, so its fullscreen window never counts, nor does a fullscreen app behind it. Checked on focus changes (and again 600 ms later, as apps go fullscreen just after coming to the front), on `ABN_FULLSCREENAPP`, and every 2 s while a panel is hidden or Explorer says a display is fullscreen. Explorer's flag alone isn't trusted: a full-screen click-through overlay can set it.
  - **Focused window:** from the helper's `foreground` events; only visible `app`/`own` windows are shown. The exe icon comes from `icons.exeIconDataUrl`. While the grid is open the panel shows (and its zones act on) the window the grid is moving, since the grid itself has the focus.
  - **Launchpad's top bar:** with a panel on, the panel on the main window's display carries the window's toolbar while the window is open (shown, not minimized): the breadcrumb leads on the left, the Layouts / Settings / New Island / Add App / fullscreen buttons sit before the grid button. `main.js` `updateTopbarHost` reports the window (`hwnd`, display, shown, fullscreen) to `panel.setLaunchpad` on show/hide/minimize/restore/move/resize/fullscreen and display changes; the window pushes its breadcrumb (`topbar:setCrumbs`). The panel's clicks come back as `panel:launchpad`. Crumbs take drops of items dragged from the window (`application/x-launchpad-id(s)`, moved by the window's `moveItemsInto`), and the current island's crumb opens its menu in the window. `panel.hostState()` → `topbar:state` tells the window to hide its own toolbar (`data-topbar-hosted`); that follows the display, not whether the window is open, so it doesn't change, and the window doesn't jump, as the window is hidden and shown. When the window is fullscreen under a docked panel, `inset` (32) moves its content (`--topbar-inset`) below the panel. With the organizer or the panel off, the window's toolbar is back.
  - There's no "active layout per display" yet; the panel shows every visible layout.
- **Grid placer** (`lib/grid.js` windows, `renderer/grid.*`, logic in `organizer.js`), gTile-style. Opened for the focused window by the grid hotkey (on the display that window is on) or a panel's grid button (on that panel's display); asking again where it's open closes it, and a panel on another display moves it there.
  1. `openGrid` asks the helper for the window's monitor and frame, picks the display, and opens a 6 × 4 grid (4 × 6 on a portrait display) over its work area. The page starts the selection on the cells nearest the window's frame (`blockFromRect`), or in the middle if it's minimized or elsewhere (`defaultBlock`).
  2. The page lays the cells out with CSS grid using the organizer's gap as gap and padding, so a block covers the rect the window will get. Drag across cells, or click one corner then the other; arrows move the selection, Shift+arrows resize it (right/down grow, left/up shrink, top-left fixed), Enter or Space places it, Esc or right-click cancels.
  3. `grid:place` validates the block as a one-zone layout of the grid and places it with the same `placeInZone` as the panel. On success, and on Esc, `closeGrid(true)` gives the window the focus back through the helper *before* hiding the grid: Windows only lets the foreground process, or one it started like the helper, bring another window forward. Losing the focus (`blur`) closes it without moving anything; `placing` ignores the blur that restoring a maximized window causes.
  - Unlike the panel and overlays, the grid window takes the focus (it needs the keyboard). It's `type: 'toolbar'` (WS_EX_TOOLWINDOW), which keeps it out of Alt+Tab and out of the helper's fullscreen check. One per display, created on first use and kept hidden; thrown away on display changes and when the organizer is turned off.
- **Scenes** (`lib/scenes.js` pure model, `lib/scene-runner.js` against real windows, `lib/scenes-menu.js` the menu window): a scene remembers each window's app, title, display and spot (§4.4).
  - **Saving** (`sceneRunner.save`) lists the helper's open windows (`windows`; the ones `dragStart` would be sent for, not Launchpad's or the shell's), leaves out minimized ones, and keeps where each is as fractions of its display's work area; maximized ones are remembered as maximized. From an island, only windows of the island's apps (matched by exe, `scenes.appExe`: `.exe` targets only), replacing the island's last scene.
  - **Applying** (`sceneRunner.apply`) pairs the scene's windows with open ones (`scenes.matchWindows`: same exe, same title first, then z-order; each used once), and places each with `place` (`max` for maximized; one already maximized on the right monitor is left alone, so it doesn't flicker). Minimized windows are restored. It resolves to what was placed, the apps with no open window and those Windows wouldn't let it move; `describe()` turns that into the message shown. One scene at a time.
  - **Open as scene** (island menu, `sceneRunner.openIsland`): launches (through `launchAndRecord`, so it counts for Recent) each of the island's apps that the scene has a window for and none is open, waits up to 20 s for their windows, lets them settle 1 s (apps move themselves to their last spot as they open), applies the scene, and applies it once more 1.5 s later.
  - **Panel menu:** the panel's scenes button (`panel:openScenes`, with its screen rect) opens a focusable drop-down under it (`type: 'toolbar'`, like the grid; one window, kept hidden), sized by the page (`scenesMenu:resize`). It lists the scenes (click to apply), "Save current arrangement" (named "Scene n") and "Manage scenes…" (opens the main window's Scenes dialog). Like the grid, it remembers the window the user was in, the panel keeps showing it, and it gets the focus back when the menu closes; blur closes it without that. Results show on the panel (`panel.showMessage`, up to 8 s for long ones).
  - **Scenes dialog** (main window, `#scenesModalOverlay`, from the Layouts dialog's "Scenes…" or the panel menu): rename (on change), Apply, Save again, Delete, and "Save current arrangement". App names come from the user's apps where the exe matches.
- **Layouts editor** (main window, `renderer.js` "window layouts editor", `#layoutsModalOverlay`): lists every layout, built-ins first (read-only, can be hidden or duplicated), then the user's. A user layout is edited on a cols × rows grid (1–12 each) where every cell belongs to exactly one zone: drag to select (widened so it never cuts a zone), **Merge** joins the selected zones, **Split** breaks them back into cells, and a single selected zone can be named. Changing the grid size scales zones when the new count is a multiple of the old (or the reverse with every edge surviving) and cuts them otherwise. Zone ids survive edits (the top-left part keeps the id), for scenes. Unnamed zones are saved as "Zone 1", "Zone 2"… in reading order and treated as unnamed again on load. Every change is saved at once through a queue, so quick edits can't land out of order. The footer sets the gap.
- `renderer/cells.js` holds the cell math for both pages (`cellSpan`, `moveBlock`, `resizeBlock`, `blockFromRect`, `defaultBlock`, `expandToZones`, `mergeZones`, `splitZones`, `resizeGrid`, `nameZones`/`unnameZones`). It's a plain script loaded before `renderer.js` and `grid.js`, so its functions are globals there, and it exports them for `test/cells.test.js` when `module` exists.
- The plan and the Win32 findings behind it: `spikes/organizer/README.md`.

---

## 6. Launch and system wrappers

### 6.1 `lib/launcher.js`
- `path` → `shell.openPath`, or a detached `spawn` if `args`/`cwd` are set.
- `url` → `shell.openExternal`.
- `script` → detached spawn of `interpreter` (default `python`) with target and args.
- `command` → detached spawn via `shell:true`.
- Any `.cmd`/`.bat` target is forced to `shell:true` (Node EINVAL hardening).

### 6.2 PowerShell helpers (`lib/*.js` → `lib/*.ps1`)
Each `.js` wrapper runs `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File <ps1> -Action …` via `execFile`, parses stdout as JSON, and returns it.

| JS | PS1 | Actions |
|---|---|---|
| `volume.js` | `volume.ps1` | `get` / `set` / `mute` (system master volume) |
| `audio-devices.js` | `audio-devices.ps1` | `listDevices`, `setDevice`, `listSessions`, `setSessionVolume`, `setSessionMuted` (C# inline COM interop) |
| `drives.js` | `drives.ps1` | `list` / `eject` (USB) |
| `bluetooth.js` | `bluetooth.ps1` | `list` / `disconnect` |
| `wifi.js` | `wifi.ps1` | `list` / `connect` / `disconnect` |
| `windows.js` | `windows.ps1` | `list` / `focus` (open top-level app windows; filters cloaked UWP host windows) |
| `power.js` | `power.ps1` | `shutdown` / `restart` / `sleep` |
| `windowmgr.js` | `windowmgr.ps1` | Long-running helper: `monitor` / `place` (optional `max`) / `focus` / `windows` / `appbarSet` / `appbarRemove` (optional `ifGone`) / `fullscreen` requests, plus `ready` / `foreground` / `dragStart` / `moveEnd` events (see below) |
| (started by `windowmgr.ps1`) | `appbar-guard.ps1` | Removes the panels' AppBars if the helper dies without doing it (see below) |

**`windowmgr` is different from the others.** Compiling its C# takes ~300 ms, so it is spawned once and kept running: from startup while something needs its events (`setKeepAlive(owner, on)`; owners are `organizer` and `hotkey`), otherwise on the first Snap to request.
- **Protocol:** JSON lines both ways over stdin/stdout. Replies are matched by `id`: `{ ok: true, … }` or `{ ok: false, code }`. Events carry `ev` instead. `ready` (with the `TaskbarCreated` message id) comes first; `currentForeground()` returns the last `foreground` event. `monitor` replies with the monitor's `bounds` and `work` plus the window's visible `frame` and `minimized` (the grid placer starts on the window's own cells).
- **Lifetime:** it exits when its stdin closes, so it can't outlive Launchpad.
- **Failures:** a request that gets no reply within 5 s kills it. While any owner wants it, it's restarted 2 s after dying; otherwise the next request starts a new one. Listeners get `{ ev: 'helperExit' }` either way.
- **Foreground events:** a `SetWinEventHook` for `EVENT_SYSTEM_FOREGROUND` (all processes), plus `EVENT_OBJECT_NAMECHANGE` scoped to the foreground window's process, so tab and document title changes come through. Each event carries `kind` (`app`, `own`, `shell`, `none`), `visible`, `title`, `exe` and `snappable`. One is sent at startup.
- **AppBars:** `appbarSet` registers the window (`ABM_NEW`; if that fails because a dead helper registered it, `ABM_REMOVE` and again), then `ABM_QUERYPOS`/`ABM_SETPOS` on the top edge. It skips `ABM_SETPOS` when the rect hasn't changed, because every `SETPOS` notifies the other AppBars (the other panels included), which would answer by re-docking. On stdin EOF it removes every AppBar it registered.
- **Crash cleanup (`appbar-guard.ps1`):** Node puts every child process in a job object that Windows kills along with Launchpad, so after a crash the helper never sees EOF and Explorer keeps the space reserved. The helper therefore starts `appbar-guard.ps1` itself (Node's job lets its processes' children leave it), tells it `add <hwnd>` / `remove <hwnd>` on stdin, and when that pipe closes (the helper exited, however) the guard removes whatever is left. Explorer accepts `ABM_REMOVE` for a window that's already gone. A detached spawn from Node doesn't work: PowerShell 5.1 exits immediately without a console.
- **Killed process tree (`appbars.json`):** killing Launchpad's whole process tree (Task Manager's "End process tree", a dev tool's stop) takes the guard with it, and Explorer then keeps the space until it restarts (seen when a dev build was stopped that way). So `lib/panel.js` writes the docked panels' handles to `appbars.json` whenever that changes, `will-quit` empties it, and the next start (`releaseLeftovers`) reads it before any panel docks and sends `appbarRemove` with `ifGone` for each (skipped if that handle is a live window again). Tested: a force-stopped dev build's space was given back on its next start.
- **Fullscreen check:** for each monitor, walks the z-order from the top: the first window on it that covers it entirely (and isn't maximized, and isn't in the request's `own` list, i.e. Launchpad's main window) makes it fullscreen. Skipped on the way: shell windows, `WS_EX_NOACTIVATE`/tool/click-through windows (Launchpad's panels and overlays among them), and smaller always-on-top windows.
- **Drag events:** a `SetWinEventHook` for move/size start and end runs on the same message-loop thread and skips Launchpad's process (`-ExcludePid`).
  - `dragStart` is only sent for a window being *moved*, not resized, and only for one worth snapping: one the taskbar would show, resizable, and not elevated. Asking the window what's under the cursor proved unreliable, because events arrive a moment late and a fast hand has already left the title bar or border. So the helper watches the window's own location changes (a hook scoped to that process) until it moves without resizing. Size-only changes are ignored, which also covers a snapped window restoring its size as the drag starts. Maximized windows count as moved straight away.
  - `moveEnd.cancelled` is true when the primary mouse button is still down as the loop ends, which is how Esc shows up.
- **DPI:** it sets per-monitor DPI awareness on its threads and works in physical pixels.
- **Placing:**
  - It positions the visible frame (DWM extended frame bounds), not `GetWindowRect`, which includes ~7 px invisible borders.
  - It restores maximized or minimized windows first.
  - It makes a second pass if the move crossed a DPI change.
- **Encoding:** it writes UTF-8 (`Console.OutputEncoding`).

`lib/unpacked-path.js` rewrites `app.asar` → `app.asar.unpacked` so child PowerShell can read the scripts. `package.json` `asarUnpack: ["lib/*.ps1"]` makes this work in the packaged build.

Other utilities: `lib/safe-file.js` (atomic writes, corrupt-copy naming), `lib/favicon.js` (pure: URL normalizing, icon `<link>` parsing, ICO decoding), `lib/icons.js` (icon extraction, favicon fetch, custom icons), `lib/drives.js`, `lib/bluetooth.js`, `lib/wifi.js`, and `lib/power.js` are the wrappers listed above.

---

## 7. Renderer (`renderer/`)

### 7.1 `index.html`
Single page. Regions:
- **Toolbar** (`.toolbar`): breadcrumb, Media Mode (§7.5), Layouts (window layouts editor, §5.1), settings, "+ New Island", "+ Add App", fullscreen. Hidden while the organizer's panel carries it (§5.1, "Launchpad's top bar").
- **Grid** (`#grid` → `#canvas`): the launcher surface. `#empty` shows the empty state.
- **Taskbar** (`#taskbar`): left = pinned row + divider + open-windows row; center = search (`#taskbarSearch`, `#searchResults`) + Recent (`#recentPopout`); right = tray widgets (volume, USB, Bluetooth, network, clock, power). Each tray widget has `data-tray-id` and is draggable to reorder.
- **Modals:** add/edit app (`#modalOverlay`), name/color (`#nameModalOverlay`), move-to-island (`#moveModalOverlay`), settings (`#settingsModalOverlay`), window layouts (`#layoutsModalOverlay`), scenes (`#scenesModalOverlay`), generic confirm/alert (`#dialogOverlay`, last so it sits above the others).
- `cells.js` is loaded before `renderer.js` (see §5.1).
- **Context menu:** `#contextMenu` (item, bulk, grid, and taskbar variants).

### 7.2 `renderer.js` (plain top-level script, ~2,900 lines), by section
1. Early pre-paint setup of theme, accent, wallpaper, taskbar and tray styles from `localStorage` (top of file).
2. DOM refs and constants (`CELL = 112`, `CARD_MARGIN`, `ISLAND_COLORS`, island header/body sizes).
3. State: `apps`, `currentFolderId`, `selectedIds`, `editingId`, modal state, `currentPinnedOrder`, etc.
4. Helpers and icon set (`folderSVG`, `ICONS`, `iconNode`, `getAncestors`, `getPathLabel`).
5. Grid: `renderBreadcrumb`, `buildTile`, `buildIslandCard`, `attachResizeHandle`, `renderCanvas`, `render`. Drag-and-drop with `addDropTarget` and `setDragData`.
6. Marquee selection, keyboard navigation (`moveSelection`, `activateSelected`), dialogs (`showAlert`, `showConfirm`).
7. External file drop from Explorer → `quickAddFromPath`.
8. Taskbar: `renderPinned` (drag-reorder pins, insert line), `renderTrayOrder` and `placeTaskbarZones` (tray drag-reorder), `renderResultRows` and `selectSearchResult` (search/recent), `positionPopoutToFit`.
9. Tray widgets, each with a `refreshX` poll and a popout: clock, network (`refreshNetworkList`), volume (`refreshVolumeState`, `refreshSessionMixer`, debounced set), USB (`refreshUsbList`), Bluetooth (`refreshBluetoothList`), power, fullscreen toggle, open windows (`refreshOpenWindows`).
   - Right-clicking an open-window tile opens a "Snap to" menu: the window's title, then one thumbnail per layout whose zones are buttons (`buildSnapPicker` → `snapOpenWindow` → `placeWindow`).
   - Layouts are fetched once into `snapLayouts`. `openContextMenu` accepts `{ node }` items for custom content like this.
10. Context menus: `showItemContextMenu`, `showBulkContextMenu`, `showGridContextMenu`, `showTaskbarContextMenu`.
11. Modals: new island and rename (`openNewIslandModal`, `openRenameModal`), move (`openMoveModal`, `openBulkMoveModal`), settings (opened by the `settingsBtn` click handler; update status via `renderUpdateStatus`/`checkForUpdates`, hotkey capture for both hotkeys via `captureHotkey`/`acceleratorFromEvent`, bar-style setters `setTaskbarPosition`/`setTaskbarEdge`/`setTaskbarAlign`/`setTrayAlign`), window layouts editor (`openLayoutsModal`, `selectLayout`, `changeDraft`, `queueLayoutChange`), add/edit app (`openAddModal`, `openEditModal`, `pickAndFillTarget`).
   - "Snap to" keeps its layouts in `snapLayouts`, fetched by `refreshSnapLayouts()` at startup and after every layout change or backup import.
12. Escape handling (last section).

Rendering pattern: the renderer keeps a local `apps` cache. Each mutation calls `refreshFromServer(window.launcherAPI.X(...))`, which replaces the cache with the `decorate()`d list returned by main, then re-renders. If main throws, `refreshFromServer` re-renders from the unchanged cache (undoing any optimistic drag) and shows the error via `showAlert`. `ipcErrorMessage()` strips Electron's "Error invoking remote method" prefix.

### 7.3 Organizer windows
- `overlay.html/.js/.css`: one per display, over its work area, click-through. Draws the drag strip and the full-size zone preview from `overlay:state` (preview only, no strip, when hovering a panel zone).
- `panel.html/.js/.css`: one per display. Builds the layout thumbnails with CSS grid on each layout's own cells (rebuilt only when the layouts or the display's shape change), shows the focused window, the grid and scenes buttons and a clock (and messages from main, `panel:message`), and follows `launchpad-topbar-style` (`data-style` on `<html>`). With `launchpad` in its state (`data-launchpad`), it also shows the main window's breadcrumb and toolbar buttons, using the same icon files.
- `scenes-menu.html/.js/.css`: the scenes drop-down under a panel's button; reports its height, focuses its first item, arrows move between items, Esc closes. Follows theme and accent.
- `grid.html/.js/.css`: one per display, over its work area, focusable. Draws the cells, the selection and a card naming the window (moved to the bottom when the selection touches the top row), and handles the mouse and keyboard (§5.1). Follows theme and accent from `localStorage`.
- Settings (main window): "Window organizer", "Organizer panel" (Always shown / Auto-hide / Off), "Snap windows by dragging them to the top of the screen" and "Grid placer hotkey". The dependent rows are disabled while the organizer is off. The panel's look is the "Top bar look" row.

### 7.4 Styling
`styles.css` uses CSS custom properties with dark as default and light via `[data-theme="light"]`. The accent is `--accent`. The wallpaper is `--wallpaper`. Taskbar and topbar styles are `data-style` attributes. On `#taskbar`: `data-pinned-side` (right), `data-align` (center), `data-tray-align`. On `<html>`: `data-theme`, `data-taskbar-edge` (top/left/right; absent = bottom), `data-taskbar-vertical` (left/right), `data-hide-add-buttons`, `data-hide-app-names`, `data-topbar-hosted` (toolbar on the panel), plus `--topbar-inset` (body padding-top, and the top of a vertical taskbar).

### 7.5 Media Mode (`renderer/media/`, main side in `lib/media/`)
A fullscreen front end for games and videos, opened from the toolbar's or panel's Media Mode button (`openMediaWindow()` in `main.js`; one window, sandboxed like the others). Plain scripts, loaded in order:
- `focus.js` (`window.MediaFocus`): spatial focus. Arrow keys move to the nearest visible `[data-nav]` element in that direction (distance along it plus twice the sideways drift). Text fields keep left/right, selects keep up/down. Everything focusable is a real button, input or select.
- `pad.js` (`window.MediaPad`; also `module.exports` for `test/pad.test.js`): Gamepad API polling on `requestAnimationFrame` while a pad is connected. `createPadReader()` turns snapshots into actions with the standard (Xbox) mapping (A select, B back, X quick, Y customize, LB/RB tabs, LT/RT page, Start menu, D-pad), a 35% left-stick dead zone, edge-triggered presses and movement repeat (400 ms, then 80 ms). Pads act as one. On window focus or visibility it ignores input for 500 ms and swallows buttons still held until released. The Guide button isn't exposed to pages, so nothing uses it.
- `prompts.js` (`window.MediaPrompts`): the bottom bar's actions per screen, drawn with key names or pad buttons depending on `body[data-input]` (the last input used).
- `media.js`: the screens. Home (Games / Videos / All tabs; rows: Continue, one per platform, Recently added, All videos; an info area for the focused tile), Add library folder (preview per subfolder with a platform dropdown, then `media:addLibrary`), Players and emulators (profiles, presets with detected installs, editor), Running (shown for tracked launches; Close it calls `media:stop`; `media:exited` returns to Home). Theme and accent come from the shared `localStorage`.
- `media.css`: same color tokens as `styles.css`, TV-sized type and targets, fixed grid rows so hiding the top bar (Running) doesn't move the rest.
- **Art:** tiles show the cover (games) or the banner (videos); the info area shows the cover or banner and the logo in place of the name; the banner shows faintly behind the page (`--backdrop`). Images load as `lpmedia://art/<uuid>/<slot>?v=<updatedAt>` (CSP `img-src 'self' data: lpmedia:`). `art-protocol.js` registers the scheme as privileged before `ready` and serves only what `art.resolveArtUrl()` maps: a UUID and a known slot, inside `media/`. Art comes in as PNG or JPEG only (what `nativeImage` decodes), at most 25 MB from a file or 15 MB downloaded, and is written to a temp file and renamed into place.
- **Customize** (Y on a tile, C, or right-click): one card per slot with a preview, Choose file, Clear, an image address field, and dropping a file on the preview. Each slot says whether you set it.
- **Controller setup** (top bar): every action with its button and a Change button. Change waits for the next pad button (`padControls.capture`; Esc or 8 s cancels, since every pad button is captured) and puts the action there with `MediaPad.assignButton`, which swaps with the action that had that button, so select and back never lose theirs. Maps are per pad (Gamepad id), saved through `media:saveController` and applied live with `configure({ deadZone, mappingFor })`. The prompt bar shows the active pad's real buttons. There's a dead-zone slider and Reset to defaults.

---

## 8. Build and release

- **Local build:** `build.bat` → `npm install` → `npm run dist` → `scripts/build.js`.
  - Runs `electron-builder` (NSIS installer, `win.target: nsis`, `allowToChangeInstallationDirectory`).
  - Exit code is ignored on purpose; the step often fails late on code-signing or Developer Mode, but `dist/win-unpacked` is still usable.
  - Re-applies the exe icon (`fix-exe-icon.js`).
  - Renames `Launchpad Setup X.exe` → `Launchpad-Setup-X.exe` (and `.blockmap`) to match `latest.yml`, because electron-updater reads that name.
- **Release (CI):** `.github/workflows/release.yml`. Manual `workflow_dispatch` with a `draft` input.
  - Reads the version from `package.json`.
  - Refuses to run if the `vX.Y.Z` release already exists.
  - `npm ci` → `npm test` → `npm run dist -- --publish never`. Failing tests stop the release.
  - Uploads the installer, its `.blockmap` and `latest.yml` with `gh release create`.
  - **Rule:** bump `package.json` version *before* running the workflow.
- **Auto-update target:** GitHub provider, `Jurakii/Launchpad`. The in-app updater only sees published, non-draft releases.

---

## 9. Feature checklist (what exists today)

- Launch apps, folders, URLs, Python/JS scripts and shell commands. Drag from Explorer to add. `.lnk` resolution.
- Freeform grid with islands (folders): nested, collapsible, resizable, colorable, optional name labels. Multi-select with marquee, bulk move/pin/remove.
- Auto icons (exe, favicon), custom icon picker, "Detect Missing Icons", "Restore Default Apps".
- Taskbar: pins (drag-reorder), running-window buttons (focus), search across everything, recent items, tray widgets (volume + per-app mixer + output device, USB eject, Bluetooth disconnect, Wi-Fi connect, power, clock, fullscreen), reorderable tray.
- Window snapping: right-click an open window on the taskbar to place it in a zone of a layout, on the monitor it's on. With the window organizer on (Settings), every display gets a panel along the top (always shown or auto-hiding; maximized windows stop below it, fullscreen apps other than Launchpad cover it) whose layout zones move the focused window there, plus the focused window's title, grid and scenes buttons and a clock. It's also Launchpad's top bar: the panel on Launchpad's screen carries the window's breadcrumb and toolbar buttons while it's open, and follows the top bar look. Dragging any window to the top of a display shows a strip of the layouts below the panel; dropping on a zone places it there. Esc or moving away cancels.
- Grid placer: Ctrl+Alt+G (rebindable) or the panel's grid button lays a 6 × 4 grid over the screen; drag across cells or use the arrow keys, Shift+arrows and Enter to size the focused window to them.
- Layouts editor (toolbar → Layouts): make layouts by merging and splitting cells, name zones, duplicate or delete layouts, hide any layout (built-ins included) from the panel, strip and "Snap to", and set the gap between snapped windows. Layouts are included in backups.
- Scenes: save where every open window is (all screens) from the panel's scenes button or the Scenes dialog, and apply a scene to put each of those windows back. Right-click an island to save its apps' arrangement, then "Open as scene" starts whichever of its apps aren't running and places them all. Scenes are included in backups.
- Taskbar placement: bottom/top/left/right edge, left/center alignment, tray center/shared alignment, solid/translucent/transparent bar styles, light/dark theme, accent color, custom wallpaper.
- Global hotkey (customisable; optional suspend while another app is focused). Tray icon, minimize-to-tray, start with Windows, start fullscreen.
- Export/import JSON backup (with icons). Auto-update check with notification and in-app download/install.
- Media Mode (Phase 1 of `MEDIA_MODE.md`): a fullscreen library of games and videos from your own folders, with platform detection, player and emulator profiles (mpv, VLC, RetroArch presets), launching with play counts and play time, and keyboard and controller navigation.

---

## 10. Conventions

- **IPC:** add the channel in `main.js` (`ipcMain.handle`), expose it in `preload.js`, call via `window.launcherAPI`. Keep one method per channel.
- **Mutations:** `store.load()` → modify → `store.save(apps)` → `return decorate(apps)`. Position-changing writes must go through `layout.findNearestFreeCell` with `siblingOccupiedFootprints`.
- **System calls** return `{ ok, error }` on failure (or `[]` for list calls), so the renderer never throws.
- **PowerShell wrappers:** new actions go in the `.ps1` `switch ($Action)` and a matching exported function in the `.js`. Output must be JSON on stdout.
- **Pure logic shared with a page** goes in a plain script under `renderer/` that ends with `if (typeof module !== 'undefined') module.exports = …`, so the page gets globals and `test/` can require it (`renderer/cells.js`). Main-side pure logic stays in `lib/`.
- **Comments** explain *why* (e.g. Windows quirks, Node EINVAL, asar paths). Match that density.
- **Windows-only:** don't add cross-platform branches unless asked.

## 11. Known issues and open items

1. **No UI tests.** `npm test` covers the pure/main-side modules (`store`, `settings`, `favicon`, `layout`, `zones`, `layouts`, `strip`, `autohide`, `scenes`, `media/library`, `media/players`, `media/scan`, `media/launch`) and the shared `renderer/cells.js` and `renderer/media/pad.js`, and runs in CI. The renderer, IPC handlers and PowerShell wrappers are verified manually (`npm start`).
2. **Non-ASCII text from most PowerShell helpers is garbled.** They write in the console's OEM code page, so "Café — 日本" reaches Node as "Caf� - ??" (window titles, Wi-Fi/Bluetooth/device names). `windowmgr.ps1` already writes UTF-8; the others need `[Console]::OutputEncoding` set the same way.
3. **Window organizer gaps:**
   - Untested so far: moving a window between monitors with different scaling (the second `place` pass), panels and the grid on a monitor above 100%, the "can't move an admin window" path, how drag-to-snap and the panel sit alongside Windows 11's own Snap Layouts flyout at the top edge, a Windows taskbar docked at the top, an Explorer restart (`TaskbarCreated`), and a packaged build. See `spikes/organizer/README.md`.
   - The grid placer was tested by a script driving the app, not yet with a real key press. Opened from a script, Windows doesn't really give the grid the foreground (Electron still reports it focused), so the focus handback after placing can't be seen there; a test window that genuinely had the foreground handed it back every time.
   - No keyboard shortcuts yet for moving the focused window to the next or previous zone (plan Q3); they need an active layout per display (Q4).
   - A release at the very top edge with no zone hovered leaves Windows' own maximize alone, by design.
   - Dropping onto the panel's breadcrumb was tested with a synthetic drop event, not yet with a real drag from the window to the panel.
   - Scenes match windows by exe path, so Store (UWP) apps, which all run inside `ApplicationFrameHost.exe`, can't be told apart, and island apps that are websites, scripts or commands can't be placed. Tested with Notepad and the user's own windows on two monitors at 100%; not yet across different scaling, or with an app that takes longer than 20 s to open.
   - Tested: a force-killed Launchpad gives the reserved space back (`appbar-guard.ps1`).
4. **Media Mode gaps (Phase 1):**
   - Controller input was tested with a simulated pad in the dev build, not a real controller. Pads without the standard mapping (non-XInput) report other button numbers; remapping and saving the mapping and dead zone come with the Controller setup screen (Phase 2).
   - The folder and program pickers (native dialogs) weren't driven by the test script; the requests behind them were.
   - No scraping, rescan button or library-folder management yet (Phase 3). The `video` art slot isn't used yet. Transparent images saved as JPEG (cover, banner, screenshot) turn black where they were transparent.
   - Remapping was tested with a simulated pad, not a real non-standard one.
   - Art isn't in backups, by design: restoring on another PC brings back the library and its `media` records, but no images until they're set again.
   - No dedicated Media Mode hotkey; the toolbar button and the main hotkey (while it's open) cover it.
   - Quitting Launchpad leaves running games open; tracked launches are forgotten.

---

## 12. Changelog pointers (recent commits)

- `8cee354` Launchpad
- `5748e05` Add a setting to disable the hotkey while another app is focused
- `5d1020c` Add a manual Release workflow and bump to v1.0.3
- `71ca7ba` Let the taskbar dock to any screen edge (bottom, top, left, right)
- `2447df4` Add Windows 10/11-style taskbar alignment (edge or center)

## 13. How to update this file
When you change something, update the matching section:
- New IPC channel → §3.4 table and the preload list (Media Mode channels go through `mediaHandle()` and `media-preload.js`)
- New setting → §4.2 (main) or §4.3 (renderer `localStorage`)
- New PowerShell action → §6.2
- New release step or version bump rule → §8
- Bugs fixed → remove from §11
