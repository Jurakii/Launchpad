# Launchpad — Codebase Summary

A personal Windows desktop-replacement launcher (Electron 44). It runs as a full-screen-capable window with a custom taskbar, an app/folder grid, and a set of system-tray widgets (volume, USB, Bluetooth, Wi-Fi, clock, power). It is meant for apps that don't behave well with the native taskbar or Start Menu.

- **Version:** 1.0.3 (`package.json`)
- **Repo:** `Jurakii/Launchpad` on GitHub, branch `main`
- **Platform:** Windows only (PowerShell helpers, `Clear-RecycleBin`, `shutdown.exe`, `ms-settings:`, etc.)
- **Size:** ~1,200 lines in `main.js`, ~2,950 in `renderer/renderer.js`, ~1,400 in `renderer/styles.css`. User-facing docs are in `README.md`.

> Keep this file current. When you change architecture, IPC channels, storage keys or the release process, update the matching section below.

---

## 1. Layout

```
main.js              Electron main process: window, tray, hotkey, auto-updater, ALL ipcMain handlers
preload.js           contextBridge → window.launcherAPI (the only renderer↔main surface)
lib/                 Main-process modules (store, layout, launcher, icons, favicon, settings, safe-file, + system wrappers)
lib/*.ps1            PowerShell helpers invoked by lib/*.js (asarUnpacked, see §6)
renderer/            index.html, renderer.js (all UI logic), styles.css, icons/*.svg
scripts/build.js     Wraps electron-builder; re-embeds icon; renames installer to match latest.yml
scripts/fix-exe-icon.js  Patches the exe icon post-build (safety net)
build.bat            Local build helper for Windows (npm install → npm run dist)
.github/workflows/release.yml  Manual "Release" workflow (windows-latest) → GitHub release
.github/workflows/test.yml     "Test" workflow: npm test on push to main and on PRs (windows-latest)
build/icon.ico       App/window/tray icon (electron-builder win icon)
test/                node:test unit tests (store, settings, favicon, layout); helpers.js stubs `electron` with a temp userData
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
  5. If packaged and `notifyUpdates` is on, a background `autoUpdater.checkForUpdates()`.
- Closing the window hides it to the tray by default (`minimizeToTrayOnClose !== false`). `isQuitting` distinguishes a real quit.
- `will-quit` stops the foreground watcher and calls `globalShortcut.unregisterAll()`.
- `activate` (macOS-style re-open) shows or recreates the window.

### 3.2 Window, tray, hotkey
- `createWindow()`: 960×680, min 640×420, menu bar hidden. Honours `startFullscreen`. Pushes `fullscreen-changed` events to the renderer.
- `createTray()`: tray icon. Left-click toggles the window. Context menu: Show / Start with Windows (checkbox) / Quit.
- `toggleWindow()`: hide if visible and focused, otherwise show and focus.
- **Global hotkey:** `registerHotkey(accel)` swaps the old accelerator for the new one. If the new one is claimed by another app it keeps the old one and returns false.
- **"Disable hotkey while another app is focused":** `lib/foreground.js` runs `lib/foreground.ps1` as a long-lived process. It prints `app` / `none` on focus change. `setHotkeySuspended()` unregisters or re-registers the hotkey on each change. Launchpad's own PID is excluded so the hotkey can still hide it. The watcher restarts after 2 s if it dies.
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
| Bluetooth | `system:listBluetoothDevices`, `system:disconnectBluetoothDevice` |
| Power | `system:shutdown`, `system:restart`, `system:sleep` |
| Wi-Fi | `system:listWifiNetworks`, `system:connectWifi`, `system:disconnectWifi`, `system:openNetworkSettings` |
| Misc system | `system:emptyRecycleBin` |
| Window | `window:isFullscreen`, `window:toggleFullscreen` (+ push `fullscreen-changed`) |
| Settings | `settings:getAutoLaunch/setAutoLaunch`, `getStartFullscreen/setStartFullscreen`, `getHotkey/setHotkey`, `getDisableHotkeyInApps/setDisableHotkeyInApps`, `getNotifyUpdates/setNotifyUpdates`, `getGoogleFaviconFallback/setGoogleFaviconFallback`, `getMinimizeToTrayOnClose/setMinimizeToTrayOnClose` |
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
- **Open-window icons** are cached per exe path in `windowIconCache`.
- **Backup format:** JSON array of entries, with each icon embedded as base64 `iconData` (stripped of `iconPath`). On import, icons are rewritten to `<userData>/icons/<id>.png`.

---

## 4. Data model and storage

Stored under Electron's `app.getPath('userData')`:

| File | Owner | Contents |
|---|---|---|
| `apps.json` | `lib/store.js` | JSON array of entries (apps, islands, pins, positions) |
| `apps.json.bak` | `lib/store.js` | The previous `apps.json`, copied before every save (only if it still parses) |
| `apps.corrupt-<ts>.json`, `settings.corrupt-<ts>.json` | store / settings | Unreadable files kept aside on load (see §4.1) |
| `settings.json` | `lib/settings.js` | Main-process prefs (see below) |
| `icons/<id>.png` | `lib/icons.js` | Per-item icon |

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
`hotkey` (default `CommandOrControl+Space`), `startFullscreen`, `minimizeToTrayOnClose` (default true), `notifyUpdates`, `disableHotkeyInApps`, `googleFaviconFallback` (default true; absent = on).

### 4.3 Renderer `localStorage` keys (UI-only prefs, read before first paint)
`launchpad-theme` (`light`/`dark`), `launchpad-accent`, `launchpad-wallpaper`, `launchpad-taskbar-style`, `launchpad-topbar-style`, `launchpad-taskbar-edge`, `launchpad-taskbar-position` (`right` = pinned side), `launchpad-taskbar-align` (`center`), `launchpad-tray-align` (`center`/`shared`), `launchpad-tray-order` (JSON), `launchpad-hide-add-buttons` (`1`), `launchpad-hide-app-names` (`1`).

Split of responsibility: anything the main process must know before a renderer exists lives in `settings.json`. Pure visual prefs live in `localStorage`.

---

## 5. Layout engine (`lib/layout.js`)

- Freeform, snap-to-grid canvas. Cells are integers. Each parent (Home or an island) has its own coordinate space.
- Loose apps and collapsed islands are 1×1. Expanded islands are `w×h` (default 3×2; min 2×2, max 8×6).
- `findFirstFreeCell` (reading order, 8 columns) for new items. `findNearestFreeCell` spirals outward for drops.
- Every write path in `main.js` calls `findNearestFreeCell` against `siblingOccupiedFootprints(...)`, so overlaps are resolved on the main side.

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
| `foreground.js` | `foreground.ps1` | Long-running watcher; prints `app`/`none` on focus change |

`lib/unpacked-path.js` rewrites `app.asar` → `app.asar.unpacked` so child PowerShell can read the scripts. `package.json` `asarUnpack: ["lib/*.ps1"]` makes this work in the packaged build.

Other utilities: `lib/safe-file.js` (atomic writes, corrupt-copy naming), `lib/favicon.js` (pure: URL normalizing, icon `<link>` parsing, ICO decoding), `lib/icons.js` (icon extraction, favicon fetch, custom icons), `lib/drives.js`, `lib/bluetooth.js`, `lib/wifi.js`, and `lib/power.js` are the wrappers listed above.

---

## 7. Renderer (`renderer/`)

### 7.1 `index.html`
Single page. Regions:
- **Toolbar** (`.toolbar`): breadcrumb, settings, fullscreen, "+ New Island", "+ Add App".
- **Grid** (`#grid` → `#canvas`): the launcher surface. `#empty` shows the empty state.
- **Taskbar** (`#taskbar`): left = pinned row + divider + open-windows row; center = search (`#taskbarSearch`, `#searchResults`) + Recent (`#recentPopout`); right = tray widgets (volume, USB, Bluetooth, network, clock, power). Each tray widget has `data-tray-id` and is draggable to reorder.
- **Modals:** add/edit app (`#modalOverlay`), name/color (`#nameModalOverlay`), move-to-island (`#moveModalOverlay`), settings (`#settingsModalOverlay`), generic confirm/alert (`#dialogOverlay`).
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
10. Context menus: `showItemContextMenu`, `showBulkContextMenu`, `showGridContextMenu`, `showTaskbarContextMenu`.
11. Modals: new island and rename (`openNewIslandModal`, `openRenameModal`), move (`openMoveModal`, `openBulkMoveModal`), settings (opened by the `settingsBtn` click handler; update status via `renderUpdateStatus`/`checkForUpdates`, hotkey capture via `acceleratorFromEvent`, bar-style setters `setTaskbarPosition`/`setTaskbarEdge`/`setTaskbarAlign`/`setTrayAlign`), add/edit app (`openAddModal`, `openEditModal`, `pickAndFillTarget`).
12. Escape handling (last section).

Rendering pattern: the renderer keeps a local `apps` cache. Each mutation calls `refreshFromServer(window.launcherAPI.X(...))`, which replaces the cache with the `decorate()`d list returned by main, then re-renders. If main throws, `refreshFromServer` re-renders from the unchanged cache (undoing any optimistic drag) and shows the error via `showAlert`. `ipcErrorMessage()` strips Electron's "Error invoking remote method" prefix.

### 7.3 Styling
`styles.css` uses CSS custom properties with dark as default and light via `[data-theme="light"]`. The accent is `--accent`. The wallpaper is `--wallpaper`. Taskbar and topbar styles are `data-style` attributes. On `#taskbar`: `data-pinned-side` (right), `data-align` (center), `data-tray-align`. On `<html>`: `data-theme`, `data-taskbar-edge` (top/left/right; absent = bottom), `data-taskbar-vertical` (left/right), `data-hide-add-buttons`, `data-hide-app-names`.

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
- Taskbar placement: bottom/top/left/right edge, left/center alignment, tray center/shared alignment, solid/translucent/transparent bar styles, light/dark theme, accent color, custom wallpaper.
- Global hotkey (customisable; optional suspend while another app is focused). Tray icon, minimize-to-tray, start with Windows, start fullscreen.
- Export/import JSON backup (with icons). Auto-update check with notification and in-app download/install.

---

## 10. Conventions

- **IPC:** add the channel in `main.js` (`ipcMain.handle`), expose it in `preload.js`, call via `window.launcherAPI`. Keep one method per channel.
- **Mutations:** `store.load()` → modify → `store.save(apps)` → `return decorate(apps)`. Position-changing writes must go through `layout.findNearestFreeCell` with `siblingOccupiedFootprints`.
- **System calls** return `{ ok, error }` on failure (or `[]` for list calls), so the renderer never throws.
- **PowerShell wrappers:** new actions go in the `.ps1` `switch ($Action)` and a matching exported function in the `.js`. Output must be JSON on stdout.
- **Comments** explain *why* (e.g. Windows quirks, Node EINVAL, asar paths). Match that density.
- **Windows-only:** don't add cross-platform branches unless asked.

## 11. Known issues and open items

1. **No UI tests.** `npm test` covers the pure/main-side modules (`store`, `settings`, `favicon`, `layout`) and runs in CI. The renderer, IPC handlers and PowerShell wrappers are verified manually (`npm start`).

---

## 12. Changelog pointers (recent commits)

- `8cee354` Launchpad
- `5748e05` Add a setting to disable the hotkey while another app is focused
- `5d1020c` Add a manual Release workflow and bump to v1.0.3
- `71ca7ba` Let the taskbar dock to any screen edge (bottom, top, left, right)
- `2447df4` Add Windows 10/11-style taskbar alignment (edge or center)

## 13. How to update this file
When you change something, update the matching section:
- New IPC channel → §3.4 table and the preload list
- New setting → §4.2 (main) or §4.3 (renderer `localStorage`)
- New PowerShell action → §6.2
- New release step or version bump rule → §8
- Bugs fixed → remove from §11
