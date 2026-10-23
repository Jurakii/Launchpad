# Launchpad

A launcher and taskbar for Windows, for apps that don't sit well in the native taskbar or Start Menu: portable apps, scripts, dev commands, websites, games.

It runs as its own window (or full screen). The window has a grid of your apps grouped into collapsible "islands", plus its own taskbar with pins, open windows, search and tray widgets. A global hotkey shows and hides it from anywhere.

## Features

- **Launch anything:** programs, files, folders, websites, Python/Node scripts and shell commands. Drag files or shortcuts in from Explorer to add them. `.lnk` shortcuts are resolved to the real target.
- **Freeform grid:** place items anywhere. Group them into islands that can be nested, collapsed, resized, colored and labelled. Select several items with a drag box to move, pin or remove them together.
- **Taskbar:** pinned items, buttons for open windows, search across everything, recently launched items, and tray widgets:
  - volume, with a per-app mixer and output-device picker
  - USB eject
  - Bluetooth
  - Wi-Fi
  - clock
  - power
- **Window snapping:** right-click an open window on the taskbar to snap it into a half, third, two thirds, main + stack or quarter of its screen. Turn on **Window organizer** in Settings for a panel along the top of each screen: click a zone in it to move the window you're using there, or drag any window to the top of a screen and drop it on a zone. The panel shows the window's title and a clock, can auto-hide, and stays out of the way of fullscreen apps. Maximized windows stop below it. It also becomes Launchpad's top bar: on the screen Launchpad is on, the breadcrumb and the Layouts, Settings, New Island, Add App and fullscreen buttons move into it, and it follows the top bar look.
  - **Grid placer:** press **Ctrl+Alt+G** (or the grid button on the panel) to lay a grid over the screen, then drag across cells, or use the arrow keys and Enter, to size the window to them.
  - **Your own layouts:** **Layouts** in the toolbar lets you make layouts by merging and splitting cells, hide the ones you don't use, and set the gap between snapped windows.
  - **Scenes:** the scenes button on the panel saves where all your windows are, on every screen, and puts them back later with one click. Right-click an island and choose **Save arrangement for this island**; then **Open as scene** starts whichever of its apps aren't running and puts each window in its place.
- **Media:** a full-screen library of your games and videos, made to use from the couch with a controller or the keyboard. Add a folder and Launchpad finds the ROMs and videos in it, works out the console from folder names and file types, and lists them by platform with a "Continue" row for what you played last. Each game or video opens in the player or emulator you choose (presets for mpv, VLC and RetroArch), and Launchpad counts plays and time played. Give anything its own cover, banner, logo and screenshot from a file, a drop or a web address, and remap your controller's buttons.
- **Layout and look:** dock the taskbar to any screen edge. Align it to the edge or center (Windows 10 or 11 style), and reorder the tray icons. Choose light or dark, an accent color, solid, translucent or transparent bars, and a wallpaper.
- **Icons:** Launchpad picks icons automatically from the program or website. You can also choose your own.
- **Backups and updates:** export and import your whole setup, icons, window layouts and scenes included. Updates install from inside the app.

## Install

1. Download `Launchpad-Setup-<version>.exe` from the [latest release](https://github.com/Jurakii/Launchpad/releases/latest).
2. Run it. The installer isn't code-signed, so Windows SmartScreen may warn you. Click **More info → Run anyway**.

Settings shows when an update is available and installs it for you. You can also turn on a notification for new versions.

## Using it

- **Ctrl+Space** shows or hides Launchpad. You can change this in Settings, or have it ignored while another app is focused, so games and editors get the keys.
- Closing the window hides it to the system tray. To exit fully, choose **Quit** from the tray icon.
- **+ Add App** adds a program, file, folder, website, script or command. Or drag something in from Explorer.
- **+ New Island** creates a group. Drag items onto it, and right-click it to rename, recolor, expand or show names.
- Right-click an item, empty grid space, or the taskbar for more options.
- With the window organizer on, **Ctrl+Alt+G** opens the grid placer for the window you're using: arrow keys move the selection, **Shift**+arrows resize it, **Enter** places the window and **Esc** cancels. You can change the keys in Settings.
- Keyboard: use the arrow keys to move, **Enter** to launch, and **Esc** to close menus. Start typing to search.
- **Media** in the top bar opens the full-screen library:
  1. **Players and emulators**: add the programs that open your files. Pick a preset or your own program, give the arguments (`{file}` is the game or video), and the platforms it's for (for example `snes`, or `videos` for films).
  2. **Add folder**: choose a folder of games or videos, check the platform found for each subfolder, and add it.
  3. Move with the arrow keys or a controller's D-pad or left stick. **Enter** or **A** launches, **Esc** or **B** goes back, and **LB**/**RB** switch between Games, Videos and All. While a game runs, **Ctrl+Space** brings Media back.
  4. **Customize art**: press **Y** (or **C**, or right-click) on a game or video to set its cover, banner, logo and screenshot. Choose a PNG or JPEG file, drop one on a slot, or paste an image's web address.
  5. **Controller**: change which button does what, per controller, and how far the stick must move before it counts.

## Your data

Everything is stored in `%APPDATA%\launchpad`:

| File | What it is |
|---|---|
| `apps.json` | Your items, islands, pins and layout |
| `apps.json.bak` | The previous version of `apps.json`, kept as a safety net |
| `settings.json` | App settings (hotkey, startup, update notifications, …) |
| `layouts.json` | Your own window layouts, which layouts are hidden, and the gap between snapped windows. Optional: the built-in layouts don't need it. |
| `icons\` | One PNG per item |
| `library.json` | Media's games and videos, with play counts. `library.json.bak` is the previous version |
| `profiles.json` | Media's players and emulators, and which one each platform uses |
| `media\` | Media's covers, banners, logos and screenshots, one folder per game or video |

Visual preferences (theme, accent color, wallpaper, taskbar layout) are stored inside the app itself.

If `apps.json` ever gets damaged, Launchpad keeps a copy of the damaged file and restores the previous version. If that isn't possible, it opens without saving changes, so nothing gets overwritten. Use **Settings → Export Config…** for a backup you can keep somewhere else. It includes Media's library and players, but not its artwork; copy the `media` folder to keep that.

**Network use:**
- Update checks go to GitHub.
- Website icons are fetched from the website itself. If a site doesn't provide one, Launchpad asks Google's favicon service instead. You can turn that off in Settings.

## Building from source

You need Windows 10 or 11 and [Node.js](https://nodejs.org/) 22 or newer.

```bash
npm install
```

```bash
npm start
```

Run the tests with:

```bash
npm test
```

To build the installer, run `build.bat`, or `npm run dist`. Output goes to `dist\`. If the installer step fails, turn on Windows Developer Mode (**Settings → Privacy & security → For developers**). Even when it fails, a working app is still left in `dist\win-unpacked\`.

### Releasing

1. Bump `version` in `package.json` and push.
2. In GitHub, go to **Actions → Release → Run workflow**. It builds the installer and publishes a `v<version>` release with the files the in-app updater needs. Drafts are ignored by the updater until you publish them.

## Project notes

[CODEBASE_SUMMARY.md](CODEBASE_SUMMARY.md) covers how the code is organized: processes, IPC channels, the data model, the layout engine and the release process.
