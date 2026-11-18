# Launchpad feature checklist

Every feature, one row each. This is the single list: `RUST_PORT_PLAN.md` and `CODEBASE_SUMMARY.md` §9 point here. Add a row when a feature is added.

Tags: **[confirm]** changes the machine or can't be undone, so stop at the confirmation and ask the person before going through with it. **[hardware]** needs something this PC may not have; if it's missing, report the row as "not run" with the reason.

"Check" is what must be true. For file checks, `<data>` is the throwaway data folder.

## X. App shell

| ID | Feature | Check |
|---|---|---|
| X1 | First run | Empty `<data>` → window opens with File Explorer at cell 0,0 and Recycle Bin at 1,0; `apps.json` is written |
| X2 | Global hotkey | The hotkey shows the window from another app, and hides it again when it's focused |
| X3 | Change hotkey | Settings → Hotkeys: capture a new combo; old one stops working, new one works; a combo held by another app is refused and the old one kept |
| X4 | Hotkey off in apps | With "disable while another app is focused" on: no effect from Notepad, works from the desktop and from Launchpad itself |
| X5 | Tray icon | Left-click toggles the window; menu has Show, Start with Windows (checkbox), Quit |
| X6 | Close to tray | Closing hides to tray; with the setting off, closing quits (no process left) |
| X7 | Single instance | Starting a second copy with the same data folder shows the first one's window and exits |
| X8 | Start with Windows | Toggling it adds/removes the Run entry (check `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`); put it back as found |
| X9 | Start fullscreen | With the setting on, next start is fullscreen; fullscreen button and state stay in sync |
| X10 | Focus hand-back | Show Launchpad over Notepad, hide it: Notepad has the keyboard again |

## L. Items and launching

| ID | Feature | Check |
|---|---|---|
| L1 | Add program | + Add App → pick `notepad.exe` → tile with Notepad's icon; clicking it opens Notepad |
| L2 | Add with args / cwd | A program with arguments and a working folder starts with both |
| L3 | Add website | `https://example.com` → tile gets the site's icon; opens in the default browser |
| L4 | Favicon fallback | A site with no icon: Google fallback used when on, no network call to Google when off |
| L5 | Add script | A `.py` or `.js` with an interpreter runs (write a script that creates a marker file) |
| L6 | Add command | A shell command runs (`cmd /c echo ok > marker.txt`) |
| L7 | Shortcut | Adding a `.lnk` stores the real target, arguments and folder |
| L8 | Edit / remove | Edit changes name, target, icon; Remove deletes the tile and `icons/<id>.png` |
| L9 | Custom icon | Pick an image → tile uses it at 128 px; Detect Missing Icons doesn't replace it |
| L10 | Detect missing icons | Delete an `icons/*.png`, run it: icon is back |
| L11 | Missing target | A tile whose target was deleted is shown as missing and says so when clicked |
| L12 | Restore / repair defaults | Remove Explorer, break Recycle Bin's target: Restore adds the missing one, Repair fixes the broken one and keeps name, place and pin |
| L13 | Recycle Bin | Tile opens it; its menu has Empty Recycle Bin **[confirm]** |
| L14 | Launch counts | Launching updates `lastLaunched` and `launchCount`; the item appears in Recent |

## F. Files on the grid

| ID | Feature | Check |
|---|---|---|
| F1 | Drop a file | Drag a `.txt` from Explorer → copied to `<data>/files/<id>/`, tile has the stored dot; still opens after the original is deleted |
| F2 | Drop a picture | Tile shows a thumbnail (`files/thumbs/<id>.png`) |
| F3 | Drop a folder | Copied recursively; opens in Explorer; folder glyph, not a drive icon |
| F4 | Drop a program | `.exe`, `.lnk`, `.bat`, `.py` are linked, not copied |
| F5 | Alt-drop / setting | Alt while dropping, or "link" in Settings, links everything |
| F6 | Too large | Over the limit (set `fileStoreLimit` small in `settings.json`): offered as a link, nothing half-copied left in `files/` |
| F7 | Same file twice | Second drop becomes "name (2)" |
| F8 | Stored menu | Show original location, Add as link, Refresh thumbnail each work |
| F9 | Remove stored | Deletes Launchpad's copy and thumbnail; the original is untouched |

## G. Grid and islands

| ID | Feature | Check |
|---|---|---|
| G1 | Free placement | Drag a tile to an empty cell: stays there after restart |
| G2 | No overlaps | Drop onto an occupied cell: lands in the nearest free one |
| G3 | New island | + New Island with name and colour; appears collapsed |
| G4 | Into / out of island | Drag a tile onto an island; drag one out of an expanded island to Home |
| G5 | Nested islands | An island inside an island; breadcrumb shows the path and each crumb navigates |
| G6 | Expand / collapse | Expanded island shows its tiles in place; state survives restart |
| G7 | Resize island | Drag the handle: guide shows, snaps between 2×2 and 8×6, neighbours aren't covered |
| G8 | Rename, recolour, show names | Each from the island's menu |
| G9 | Remove island | Its children move up to the parent and get new cells |
| G10 | Marquee select | Drag a box over several tiles; bulk Move to, Pin, Remove act on all |
| G11 | Move to… | Right-click → Move to lists Home and every island |
| G12 | Keyboard | Arrows move the selection, Enter launches, Esc closes menus, typing starts search |
| G13 | Grid menu | Right-click empty space: + Add App, + New Island |
| G14 | Empty state | With no items, the empty message shows |

## T. Taskbar

| ID | Feature | Check |
|---|---|---|
| T1 | Pin / unpin | Pin from the menu; appears on the bar; unpin removes it |
| T2 | Reorder pins | Drag a pin; insert line shows; order survives restart |
| T3 | Open windows | Open Notepad: a button appears; clicking focuses it; closing removes it. A title with non-ASCII characters (`Café 日本.txt`) shows correctly |
| T4 | Search | Typing finds items in every island; Enter launches the first; arrows move |
| T5 | Recent | Lists the last launched items, newest first |
| T6 | Snap to | Right-click an open window → pick a zone of a layout: the window fills that zone on its own monitor |
| T7 | Bar menu | Right-click the bar: edge, alignment and tray alignment options, Taskbar Settings… |

## W. Tray widgets

| ID | Feature | Check |
|---|---|---|
| W1 | Clock | Right time and date; updates on the minute |
| W2 | Volume | Slider and mute match Windows' own; changing either side updates the other |
| W3 | Per-app mixer | A playing app is listed; its slider and mute affect only it |
| W4 | Output device | Lists the devices; choosing one changes Windows' default **[confirm]** (put it back) |
| W5 | USB list | A plugged-in USB drive is listed with its name **[hardware]** |
| W6 | USB eject | Eject removes it safely **[confirm] [hardware]** |
| W7 | Bluetooth list | Paired devices with connected state **[hardware]** |
| W8 | Bluetooth settings link | "Bluetooth settings…" opens Windows' Bluetooth page. (1.2's Disconnect button is gone in 1.3: it needed admin rights and never worked.) |
| W9 | Wi-Fi list | Networks with signal and the current one marked **[hardware]** |
| W10 | Wi-Fi connect / disconnect | Works for a saved network **[confirm] [hardware]**; Network settings opens Windows' page |
| W11 | Power | Menu shows Shut down, Restart, Sleep; each asks first. Never go past the question without a person saying yes **[confirm]** |
| W12 | Reorder tray | Drag widgets to reorder; survives restart |
| W13 | Popouts | Each opens next to its icon, stays on screen at every bar edge, closes on Esc or outside click; a widget can't be dragged while its popout is open |

## P. Look and layout

| ID | Feature | Check |
|---|---|---|
| P1 | Theme | Light and dark apply to every window (main, panel, grid, menus, Media) |
| P2 | Accent | Changing it recolours highlights everywhere, at once |
| P3 | Wallpaper | Set and clear a wallpaper image |
| P4 | Bar styles | Solid, translucent, transparent for the taskbar and for the top bar |
| P5 | Bar edge | Bottom, top, left, right; popouts and tooltips follow |
| P6 | Alignment | Edge or centre for buttons; tray centre / shared / edge |
| P7 | Hide add buttons / names | Each setting hides what it says |
| P8 | Settings dialog | Sections switch; search finds a setting in another section; last section is remembered |
| P9 | No flash | A restart with light theme and a wallpaper never shows the dark default first |

## O. Window organizer

Turn on Settings → Window organizer first. Use Notepad windows as the subjects.

| ID | Feature | Check |
|---|---|---|
| O1 | Panel docks | A 32 px bar on top of every display; a maximized window stops below it |
| O2 | Panel off / quit | Turning the organizer off, or quitting, gives the space back (maximize a window to see) |
| O3 | Kill recovery | Kill the process with the panel docked; start again: space is given back, then re-docked |
| O4 | Focused window | Panel shows the focused window's icon and title, and follows title changes |
| O5 | Zone click | Clicking a zone opens the window picker with nothing selected; picking a row moves that window; Esc moves nothing |
| O6 | Panel keeps focus away | Clicking the panel never takes the keyboard from the app in use |
| O7 | Hover preview | Hovering a zone shows it full size on that display |
| O8 | Auto-hide | Panel comes down after the cursor rests at the top edge, leaves after it moves away; no space reserved |
| O9 | Fullscreen | A fullscreen video or game covers the panel; Launchpad's own fullscreen doesn't hide it |
| O10 | Drag to snap | Dragging a window to the top shows the strip; dropping on a zone places it; Esc or moving away cancels; resizing a window never shows it |
| O11 | Exact placement | Two windows in neighbouring zones are exactly the gap apart, with no invisible-border slack |
| O12 | Grid placer | Ctrl+Alt+G opens the grid on the focused window's display with its current cells selected; drag, arrows, Shift+arrows, Enter place it; Esc cancels and focus returns to the window |
| O13 | Grid hotkey | Rebind it; it can't be set to the show/hide hotkey and the reverse |
| O14 | Layouts editor | New layout: merge, split, name a zone, change grid size, duplicate, delete; built-ins can only be hidden or duplicated |
| O15 | Hide layout | A hidden layout leaves the panel, the strip and Snap to |
| O16 | Gap | Changing the gap changes the next placement |
| O17 | Hosted top bar | With the panel on, the main window's breadcrumb and buttons are on the panel on its display and each button works; with the panel off they're back in the window |
| O18 | Drop on crumb | Drag an item from the grid onto a breadcrumb on the panel: it moves there |
| O19 | Display change | Unplug or rearrange a monitor: panels follow **[hardware]** |
| O20 | Mixed scaling | Snap a window across monitors at different scaling: right size on arrival **[hardware]** |
| O21 | Elevated window | Snapping an admin window says it can't be moved **[hardware: needs an elevated app]** |
| O22 | Explorer restart | Restart `explorer.exe`: panels dock again **[confirm]** |

## S. Scenes

| ID | Feature | Check |
|---|---|---|
| S1 | Save | Panel's scenes button → Save current arrangement: a "Scene n" listing the open windows |
| S2 | Apply | Move the windows, apply: each returns to its place; the message names apps that weren't open |
| S3 | Maximized / minimized | A maximized window is restored maximized; a minimized one is brought back |
| S4 | Manage | Scenes dialog: rename, Save again, Delete, Apply |
| S5 | Island scene | Island menu → Save arrangement for this island keeps only its apps' windows |
| S6 | Open as scene | Starts the island's apps that aren't running, then places them |
| S7 | Menu behaviour | The scenes menu closes on Esc and outside click, and returns focus to the app in use |

## M. Media

| ID | Feature | Check |
|---|---|---|
| M1 | Open / close | Media button opens it fullscreen; Exit returns to the main window; the hotkey toggles Media while it's open |
| M2 | Add player | Settings → Players: add from a preset, and a custom `.exe` with `{file}` arguments; `.bat` is refused |
| M3 | Add folder | Pick a folder of ROMs/videos: preview groups by subfolder with a detected platform; Add lists them |
| M4 | Add single app | An `.exe` is added once; adding it again is refused |
| M5 | Home rows | Games / Videos / All tabs; Continue, per-platform, Recently added rows |
| M6 | Keyboard | Arrows move focus to the nearest tile, Enter launches, Esc goes back |
| M7 | Launch tracking | Launching shows Running; closing the player returns to Home with play count and time updated; Close it stops it |
| M8 | Missing file | An entry whose file is gone is shown as unavailable, not removed |
| M9 | Art | Set cover, banner, logo, screenshot from a file, a dropped file and a web address; Clear removes it; too-large or non-image input is refused |
| M10 | Colours | Change a colour token, see the contrast warning for a bad pair, Reset one and Reset all |
| M11 | Controller | A pad moves focus, A selects, B goes back, LB/RB switch tabs; prompts show pad buttons **[hardware]** |
| M12 | Remap | Controller setup: change a button, dead zone slider, Reset to defaults **[hardware]** |

## D. Data, backup, updates

| ID | Feature | Check |
|---|---|---|
| D1 | Export | Settings → Export Config writes one JSON with apps, icons, layouts, scenes, library, profiles, plus `<name>-files/` when there are stored files |
| D2 | Import | Importing it into an empty data folder brings everything back, stored files included |
| D3 | Cross-build | A backup from the Electron build imports in the Rust build and the reverse; an old bare-array backup imports |
| D4 | Corrupt `apps.json` | Break the JSON: next start restores from `.bak` and says so, keeping `apps.corrupt-*.json`. With no `.bak`: opens read-only and saves nothing |
| D5 | Corrupt settings / layouts | Moved aside to `*.corrupt-*.json`, defaults used |
| D6 | Atomic save | Kill the process during heavy editing: `apps.json` always parses |
| D7 | Old data | A data folder written by 1.2.2 opens unchanged: same items, places, pins, layouts, scenes, library |
| D8 | Update check | Settings shows the current version and whether a newer release exists (reads `launchpad-update.yml`); with notifications on, a newer one is announced at start |
| D9 | Install update | Download and install from Settings **[confirm]** |
| D10 | Reinstall | Settings → reinstall this version **[confirm]** |
| D11 | Reset | Danger zone: needs the typed confirmation, writes `pre-reset-*.json`, keeps Media; only ever run against the throwaway data folder **[confirm]** |
| D12 | Installer | Installs, starts, uninstalls cleanly; data folder is left alone **[confirm]** |
| D13 | Over 1.2.2 | Installing 1.3 by hand over a 1.2.2 install removes the Electron app, keeps the data folder, and keeps theme, accent, wallpaper and bar layout **[confirm]** |
| D14 | Not offered to 1.2.2 | With 1.3 as the newest release, 1.2.2's Settings → Check for updates does not offer it |

## B. Budgets (Rust build; record the Electron numbers for comparison)

Measure with `measure.ps1`, 30 s after reaching the state. Targets are in `RUST_PORT_PLAN.md` §2.

| ID | State | Target |
|---|---|---|
| B1 | Hidden in tray, organizer off | ≤ 15 MB private, 1 process |
| B2 | Main window open, 200 items, wallpaper | ≤ 40 MB private |
| B3 | Organizer on, main window hidden | ≤ 25 MB private |
| B4 | Media open with art | ≤ 80 MB private |
| B5 | After opening and closing every window 20 times | Back within 2 MB of B1 |
| B6 | Exe / installer size | ≤ 12 MB / ≤ 5 MB |
| B7 | Idle CPU, hidden, 60 s | 0% |
