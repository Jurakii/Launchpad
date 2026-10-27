# Launchpad — Features and Bug Fixes: Implementation Notes

Based on `CODEBASE_SUMMARY.md` (v1.0.4). Items are ordered by how self-contained they are. File references point at the code the summary describes; line numbers are from the current tree and may drift.

## Decisions

| Question | Decision |
|---|---|
| §1 Repair source | Fetch from the release tag matching the running version (`v<app.getVersion()>`), not `latest`. Otherwise it works like an update check. |
| §2 Picker selection | Nothing pre-selected. Enter does nothing until a row is chosen. |
| §3 Top-bar focus | Return focus to the previous app when Launchpad hides (the common launcher behaviour). |
| §8 Reset Media data | Not reset. Media stays as it is; the dialog says so. |
| §8 Reset open windows | Close transient and Media windows, then reset and relaunch the app. |
| §8 Reset (new) | Added below. Requires a backup prompt and a typed confirmation. |

---

## 1. Feature: Repair / reinstall the default apps from Settings

**Goal:** a Settings button that re-downloads or rebuilds the stock setup from the current version, so a user can recover after a bad install or an update.

**Current state**
- Settings already has a "Restore Default Apps" button (`renderer/index.html` ~line 447, handler `renderer/renderer.js` ~line 3224).
- It calls `apps:restoreDefaults` (`main.js` ~line 850), which only *adds* File Explorer and Recycle Bin when no app with the same name exists. It does not repair anything: a broken icon, a wrong target, or a missing `icons/<id>.png` is left as is.

**Plan**
1. Decide the scope with the user:
   - *Repair* (recommended first step): for each default entry present, re-run `autoFetchIcon` and reset `target`/`command` to the value the current version ships; add any missing defaults.
   - *Reinstall*: also re-download icons for every item the user did not customise (`iconIsCustom === false`), leaving user apps alone.
   - "Re-download from the current version" most likely means the app's own bundled assets, not the network. Confirm before building a network path.
2. Add a `repairDefaults()` in `main.js` next to `buildDefaultAppEntries()` (~line 425). It should:
   - Match defaults by a stable key (e.g. `special` for Recycle Bin, and `target === EXPLORER_PATH` for Explorer), not by name, so a renamed entry is still found.
   - Refresh `target`, `command`, `special` and `iconPath`; never touch `parentId`, `gridX/gridY`, `pinned` or `color`.
   - Return `{ apps, repaired, added }` so the UI can say what changed.
3. Add the IPC channel (`ipcMain.handle('apps:repairDefaults')`) and expose it in `preload.js` next to `restoreDefaults` (line 23).
4. UI: in `renderer/index.html` add a "Repair default apps" row beside the existing button, and wire it in `renderer.js` with the same disabled/label pattern as `restoreDefaultsBtn`. Show the result with `showAlert`.
5. Tests: add cases to `test/store.test.js` (or a new `test/defaults.test.js`) for the match-by-key logic, using the `test/helpers.js` temp userData.

**Done when:** Settings → Repair reports a count, a deliberately broken Explorer entry is fixed, and a user-renamed or user-moved item keeps its name and position.

---

## 2. Bug: Clicking a layout "template" in the panel moves the current window

**Expected:** clicking a layout zone in the organizer panel always opens the window list (the picker) so the user chooses which app to move.

**Current behaviour**
- `lib/organizer.js` `snapFromPanel` (~line 224) asks `pickerRules.panelAction(target, helperUp)`.
- `lib/picker.js` `panelAction` returns `'snap'` whenever the *last foreground window* is `snappable`. The panel is `focusable: false`, so clicking it leaves the foreground on the user's previous app. That app is usually snappable, so it gets moved without any choice.

**Fix**
1. Change `panelAction` in `lib/picker.js` so a panel zone click always returns `'pick'`. Keep `'wait'` when the helper is down.
2. Keep the direct-snap path for the **grid placer** and the **drag-to-snap strip**, which already have a clear target. Only the panel's zone click changes. Check `organizer.js` line ~338 (`gridTarget()`) before touching it, since it uses the same function.
3. Update `test/picker.test.js`: the case `panelAction snaps a snappable window straight away` (line 7) now needs to assert `'pick'` for panel clicks. Add a separate function, or a flag, if the grid still needs `'snap'`.
4. Open question for the user: should the picker still pre-select the last window (so Enter confirms it)? That would keep speed without silent moves. Recommended.

**Done when:** with Notepad focused and snappable, clicking a zone in the panel opens the list; choosing a row moves that row's window; nothing moves until a row is picked.

---

## 3. Bug: Clicking the top bar brings Launchpad forward and hides apps on a single monitor

**Cause:** the panel's top-bar actions are forwarded to the main window, and that path calls `showWindow()` (`main.js` ~line 173), which runs `show()` and `focus()` on the full-screen Launchpad window. On a single monitor it covers every app.

**Where:**
- `main.js` `topbar:action` forwarding (~line 1151, `mainWindow.webContents.send('topbar:action', message)`) after `showWindow()` in the `panel:launchpad` handler.
- Crumb clicks, `crumbMenu`, `drop`, `newIsland`, `addApp`, `settings`, `layouts`.

**Fix**
1. Split the actions by whether they need the window *visible*:
   - Navigation and data actions (`crumb`, `crumbMenu`, `drop`, `newIsland`, `addApp`) — these only make sense in the window, so keep showing it, but as a normal window: do not use `focus()` on an already-open window, and do not change its z-order when it is already visible.
   - Dialogs (`layouts`, `settings`) — open the dialog and show the window only if it is needed for it.
2. In `showWindow()` add an option, e.g. `showWindow({ focus: false })`, and use it from the panel path. Windows may refuse `focus()` anyway, so verify with the helper's `focus` if needed.
3. Check that `updateTopbarHost` (see summary §5.1) is not triggering a re-show on every move/resize.
4. Consider a "return to previous app" behaviour: remember the foreground window before showing Launchpad and restore it when the window is hidden. This is the cleanest fix for single-monitor setups.

**Done when:** on a single monitor with an app in front, clicking the breadcrumb or Settings from the panel leaves the app visible behind the Launchpad window, or Launchpad opens only where the action needs it.

---

## 4. Bug: Apps can't be dragged out of expanded islands

**Where to start:** `renderer/renderer.js`
- `buildIslandCard` (section 5 of the renderer summary) and `attachResizeHandle`.
- Drag handling: `setDragData`, `addDropTarget`, and the drop handler that calls `item:move` / `item:setPosition`.
- The island body is a nested drop target; a drop on an expanded island's child may be cancelled by the island's own drop zone, or the child's `dragstart` may be stopped by a parent handler.

**Likely fixes (verify in the debugger first)**
1. In the island's drag/drop handlers, do not call `stopPropagation()` or `preventDefault()` for drags that start on a child tile.
2. When a child is dropped on the Home canvas, call `item:move` with `parentId: null`, which clears the child's grid position (summary §3.5). Make sure the canvas drop target accepts drags whose origin is an island (the drag data should carry the source `parentId`).
3. Check that the island body does not capture the pointer; an expanded island has a larger footprint (`w×h`, summary §5), so its hit area may cover the Home canvas edge where the drop lands.

**Done when:** an app inside an expanded island can be dragged onto the Home canvas, and onto the top-bar breadcrumb, and lands on a free cell.

---

## 5. Feature: Right-click → Move to → "List home and all islands"

**Where:** `renderer/renderer.js` `showItemContextMenu` (~line 2422) and `openMoveModal` (~line 2654). The move modal already lists islands, so this is mostly a new entry.

**Plan**
1. In `showItemContextMenu`, add a "Move to" submenu (the context menu supports `node` items and submenus; check `openContextMenu`). Inside it, list:
   - "Home" (the top-level, `parentId: null`), then
   - every island, in the same order as the move modal.
2. Each entry calls `item:move` with the chosen parent, then the same refresh path as the modal.
3. Show only islands that are not the item itself or one of its descendants (`descendantIslandIds` in `main.js` ~line 489 already computes this for the server side; reuse the same rule in the renderer to avoid showing invalid targets).
4. Reuse `item:move`'s existing free-cell placement (see §6), so the moved item never lands on another icon.

**Done when:** the context menu shows a Move to submenu with Home and every island, and choosing one moves the item there.

---

## 6. Bug: Added or force-moved apps sometimes land on top of another icon

**Cause:** the placement rule in the summary (§5) says every write path calls `findNearestFreeCell` with `siblingOccupiedFootprints(...)`. Some paths do not, or they pass a footprint that is stale, so the check runs against the wrong list. Paths to check:
- `apps:add`, `quickAddFromPath`, and the file-store drop (`lib/file-store.js` line ~161 uses `findNearestFreeCell(occupied, gridX, gridY, 1, 1)`).
- `item:setPosition` / `item:setPositions` (batch): each position in the batch may be checked against the old list, not the list including earlier moves in the same batch.
- `item:move` with a forced target (`apps:update` changing `parentId`).
- `restoreDefaults` (`main.js` ~line 856) calls `assignMissingPositions`, which uses `findFirstFreeCell`. Check `lib/store.js` line ~87 too.

**Fix**
1. Add one helper, e.g. `placeFree(apps, parentId, x, y, w, h, excludeId)`, in `main.js` (or `lib/layout.js`). It must:
   - Build occupancy from `siblingOccupiedFootprints(apps, parentId, excludeId)` every time (not cached).
   - Return `findNearestFreeCell` when a target is given, and `findFirstFreeCell` when not.
2. Route every write path through it. Remove any inline placement logic.
3. In batch writes, update the in-memory `apps` after each placement, so the next item in the batch sees it.
4. Add tests to `test/layout.test.js` for the batch case (two items forced to the same cell should end on two different cells).

**Done when:** adding, dropping, moving between islands, and batch-moving never leaves two items on the same cell. A quick check: force-move one item onto another's cell and verify the moved one lands on the nearest free cell.

---

## 7. Bug: Default apps are in the wrong order on creation

**Expected:** File Explorer first, then Recycle Bin, matching how they appear in File Explorer's sidebar.

**Current:** `buildDefaultAppEntries` (`main.js` ~line 425) returns `[explorerEntry, binEntry]` with `createdAt: now` and `now + 1`. The order in the array is correct, so the reversal comes from somewhere else. Likely causes:
1. The grid sorts by `createdAt` descending, or `assignMissingPositions` (`lib/store.js` line ~87) assigns cells in reverse.
2. The seeding code (in `app.whenReady`) pushes the entries onto `apps` in reverse, or `store.save` re-sorts them.

**Fix**
1. Find the seed call in `app.whenReady` and check the array order it writes, then the `gridX/gridY` each item gets.
2. Make the placement explicit rather than relying on array order: give File Explorer `gridX: 0, gridY: 0` and Recycle Bin the next free cell (`gridX: 1, gridY: 0`) in `buildDefaultAppEntries`, and set `createdAt` so Explorer is the older item.
3. Also apply to `restoreDefaults` (§1), which must keep the same order.

**Done when:** on a fresh profile (delete `apps.json` in userData, or test with the temp userData), File Explorer is at the first cell and Recycle Bin is next to it.

---

## 8. Feature: Reset everything to defaults (Settings)

**Goal:** a Settings button that returns Launchpad to its first-run state. It must be hard to press by accident, and it should recommend a backup first.

**Scope (decided):** `settings.json` back to defaults, `apps.json` replaced by the default two apps, `layouts.json` removed (built-ins only), icons and stored file copies (`icons/`, `files/`, `thumbs/`) removed, and the renderer `localStorage` theme/layout keys cleared. Media (`library.json`, `profiles.json`, `media/` art) is **not** touched. The dialog must say so in plain words, e.g. "Your Media library, players and artwork are kept.", so the user is not surprised.

**Open windows (decided):** before resetting, close the transient windows (grid placer, scenes menu, picker) and the Media window, then reset and relaunch the app (`app.relaunch()` + `app.exit()`). A relaunch clears every in-memory cache, the window-manager helper, and any AppBar space the organizer panels hold, so nothing is left in a half-reset state. Closing the Media window is only a window close; its library stays on disk. Pick the same flow for the organizer panels: they are removed by the relaunch, not reset in place.

**Plan**
1. Settings → a "Danger zone" section at the bottom, with a red "Reset Launchpad…" button. It is not next to Restore Default Apps.
2. Clicking it opens a dialog that:
   - Says what will be deleted, using the scope above.
   - Recommends a backup and offers "Back up first" (reuses `backup:export`). The reset button stays disabled until the user either backs up in this session or ticks "I don't want a backup".
   - Requires typing `RESET` into a field before the confirm button enables.
3. Main process: `settings:reset` handler. It writes a backup of the current state to `userData/pre-reset-<timestamp>.json` first, so a reset is always recoverable even if the user skipped the export. Then it deletes or rewrites the files above with `writeFileAtomic`, and returns `{ ok }`.
4. The renderer reloads after success so every in-memory cache starts clean.
5. Tests: `test/settings.test.js` for defaults after reset; `test/backup.test.js` for the pre-reset file being written before anything is deleted. Check that a failure part-way leaves the pre-reset file in place.

**Done when:** the button sits in a danger zone, the dialog cannot be confirmed without typing `RESET`, a `pre-reset-*.json` file exists after a reset, and a restart shows a first-run install.

---

## Suggested order of work

| # | Item | Size | Risk |
|---|---|---|---|
| 7 | Default app order | small | low |
| 6 | Free-cell placement everywhere | medium | medium (touches every write path) |
| 2 | Panel zone click always picks a window | small | low, changes behaviour |
| 3 | Top bar stops raising Launchpad | medium | medium (window focus rules are Windows-specific) |
| 4 | Drag out of expanded islands | medium | low, renderer only |
| 5 | Move to submenu | small | low, depends on §6 |
| 1 | Repair default apps (from the version's own release tag) | medium | low |
| 8 | Reset everything (danger zone, backup first) | medium | medium (deletes user data) |

Do §6 before §5, since the new Move to entry should use the same placement helper.

## Testing

- Unit: `npm test` (runs `test/*.test.js`). Add cases for §1 (defaults matching), §2 (`panelAction`), §6 (batch placement) and §7 (seed positions).
- Manual on a single monitor: §3 and §2 need a real window; they cannot be checked from unit tests.
- Manual, fresh profile: §7. Remove the userData `apps.json` before launching.

## Open questions for the user

Answered (see Decisions and §8). No open questions remain for the plan.
