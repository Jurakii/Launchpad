# Media Mode: Research and Implementation Plan

Status: Phase 1 is built (2026-10-23): library store, profiles, folder scanning, tracked launching, the Media Mode window and controller input. `CODEBASE_SUMMARY.md` §4.5 and §7.5 describe what exists; §11 there lists Phase 1's known gaps. Phases 2 to 4 are not started. §9 lists what the pre-build review changed.

Designs: [Media Mode design canvas](https://claude.ai/artifact/8x1EMf45s551xxEnxkUEz5) (Home, Videos, Scraper review, Customize art, Controller setup, Players and emulators, Add library folder, Game running).

## 1. Goal

Add a **Media Mode**: a fullscreen, controller- and remote-friendly front end, in the spirit of Steam Big Picture or EmulationStation, that lets the user:

1. Launch **videos and media** through a chosen media player (mpv, VLC, MPC-HC, Kodi, or any command line).
2. Launch **retro and modern games**, both native PC games and emulated ROMs, through the user's emulators.
3. **Customize covers and banners** for every entry, by hand or from a scraper.
4. **Auto-fill metadata and artwork** with scrapers, similar to EmulationStation / ES-DE.

Media Mode should reuse Launchpad's existing machinery (item model, `launcher.js` spawn/openPath path, icon cache, settings, backups) instead of being a separate app.

## 2. What Launchpad already has

Relevant pieces from the current codebase (see `CODEBASE_SUMMARY.md`):

| Piece | Where | Reuse for Media Mode |
|---|---|---|
| Item model (`kind: 'app' / 'island'`, `name`, `type: path / url / script / command`, `target`, `args`, `cwd`, grid position, pins) | `lib/store.js`, `apps.json` | Copy its load/backup/corruption handling into a **separate** `library.json` store. Do not put library entries in `apps.json` (§5.1 says why). Reuse its field names. |
| Launch pipeline (`detachedSpawn`, `shell.openPath`, `openExternal`) | `lib/launcher.js` | Add a profile launch that keeps the child process for tracking; `detachedSpawn` unrefs it and returns nothing |
| Icon handling (one PNG per item in `%APPDATA%\launchpad\icons\`, sent to the renderer as data URLs) | `lib/icons.js` | Reuse `nativeImage` loading and the capped image fetch (`readCapped`, `fetchImage`). Covers need a different delivery path (§5.4). |
| Settings (`settings.json`, merged top-level `save(patch)`, atomic writes) | `lib/settings.js`, `lib/safe-file.js` | Controller mapping and Media Mode preferences. Not scraper secrets (§4). |
| Backups (`backup:export` writes one JSON with every icon embedded as base64) | `main.js` | Add the library and profiles; leave art out (§5.1) |
| Strict renderer ↔ main boundary (`preload.js`, `contextBridge`) | `preload.js` | All file access, scraping and launching stays in main; the Media Mode UI only calls IPC |
| Gamepad-free UI today | `renderer/` | Media Mode needs its own renderer surface (see §5) |

## 3. How the reference products do it

### 3.1 Steam Big Picture
- A separate fullscreen UI shell over the same library data.
- Navigation is controller-first: a horizontal row/carousel per category, large artwork tiles, and an always-visible focus ring.
- Per-title artwork types: capsule (portrait), header (banner), hero (background), logo. Users can replace any of them, and the app keeps a local cache.
- Launching hands off to the game's executable and tracks the running process.

### 3.2 EmulationStation / ES-DE
- Systems (consoles) are the top level. Each system has a `gamelist.xml` (or `gamelist.xml` equivalent metadata) and a `downloaded_media/` folder.
- Each game entry carries: name, path, description, rating, release date, developer, publisher, genre, players, and media: `marquee` (logo), `screenshot`, `thumbnail` (cover), `cover` (box art), `fanart`, `video`, `3dbox`, `physicalmedia`, `titlescreen`.
- Launching is done by an **emulator command template** per system (the `es_systems.xml` / ES-DE `custom_systems` pattern), where `%ROM%` is replaced with the file path. Emulators are configured once per system; each game just inherits that.
- Scraping is a separate pass: the user picks a source (ScreenScraper, TheGamesDB, and so on), chooses which media to fetch, and then runs a batch. Matching is by filename first, with a fallback to a search by name, and the user can review mismatches.
- Scraping is resumable and writes media next to the data, so a moved library keeps its art.

### 3.3 Playnite
- A library database with **metadata + media + launch action** per game. Launch actions are typed: an executable, a URL, an emulator profile, or a script.
- **Emulator profiles** are defined once (path, arguments with placeholders, platforms they serve) and then referenced by games. This is the same idea as ES-DE's system command, but with multiple profiles per platform.
- Metadata and media come from **metadata plugins** (for example ScreenScraper, Steam Grid DB, IGDB, LaunchBox DB). Each plugin is an independent source with its own credentials; the user picks which field comes from which source.
- Extensible through plugins and scripts.

### 3.4 Kodi / Jellyfin / Plex (for video)
- They **index** a folder and read filenames and metadata (NFO files or scrapers such as TMDb) to build a library of movies and shows, with posters and fanart.
- Playback is normally delegated to the built-in player. Launchpad should instead delegate to an **external** player, which the user has asked for.

## 4. Scraper options

| Source | What it gives | Access | Notes for Launchpad |
|---|---|---|---|
| **ScreenScraper.fr** | The most complete retro source: covers, marquees, screenshots, videos, 3D boxes, multiple regions and languages | Free account; the app needs a developer ID, and users need a user login for their quota | Best default for ROMs. Confirm the current developer-key and quota rules on the site before building; the research pass did not find them. |
| **TheGamesDB** | Text metadata, some art, good for PC titles | Free API key | Good fallback for metadata |
| **IGDB** (Twitch) | Text metadata and cover art, strong for modern and PC games | Twitch client ID + secret, client-credentials token | Needs the client secret, so it cannot ship inside the Electron renderer. Calls must run in main, and the user supplies their own credentials. Reported limit: about 4 requests/second per client ID, max 500 results per request. Verify against the official docs (api-docs.igdb.com). |
| **SteamGridDB** | User-made covers, heroes, logos and icons for any game | Free API key | Excellent for custom art; a good fit for "customize covers and banners" |
| **LaunchBox DB** | Large XML metadata dump | Download, no live API | Good offline option; very large |
| **Local NFO / sidecar files** | Whatever the user wrote | None | Needed for video libraries |
| **TMDb / OMDb** (for video) | Movie and TV metadata and posters | Free API key | Standard choice for the video side |

Design rules for scrapers:
- Every scraper is a module in `lib/scrapers/` with the same interface: `search(query, platform)`, `getDetails(id)`, `getMedia(id, types)`.
- Rate limiting and retry live in one shared queue, per source, with backoff on 429.
- Results are cached on disk, keyed by source and source ID, so a rescrape does not spend quota.
- **Credentials go in their own `secrets.json`, encrypted with Electron's `safeStorage`** and written with `writeFileAtomic`. They are never sent to the renderer after entry: a dedicated IPC returns only "configured" or "not configured" per source. Keeping them out of `settings.json` means they never end up in a settings dump or a backup.
- Scraping never overwrites user-chosen art. Each media slot records whether it came from a scraper or from the user, and the scrape respects that.

## 5. Proposed architecture

### 5.1 Data model

Library entries live in a **new store, `library.json`** (module `lib/media/library.js`), not in `apps.json`. The review found three reasons:

- `store.load()` runs `assignMissingPositions` on every entry, so each ROM would get a cell on the Home grid.
- `apps:list` sends every entry to the renderer with its icon as a data URL. A few thousand ROMs would make every grid refresh slow.
- `backup:export` embeds every icon as base64 in one JSON file. Covers would make backups hundreds of MB.

`library.js` copies `store.js`'s safety behavior: `writeFileAtomic`, a `.bak` copy, the corrupt-copy-and-restore path, and read-only mode when nothing is readable. A normal Launchpad item can later point at a library entry (for example a pinned favorite game) by id, without the two stores merging.

Field names follow `apps.json` where the meaning is the same (`id`, `name`, `type`, `target`, `args`, `cwd`, `launchCount`, `createdAt`), so code and habits carry over.

```jsonc
{
  "id": "…",                     // UUID, like apps.json; also the media folder name
  "library": "games",            // games | videos
  "name": "Chrono Trigger",
  "platform": "snes",            // games: platform id, used to pick the default profile
  "type": "profile",             // profile (new) | path | url | script | command (existing launcher types)
  "profile": null,               // profile id; null = the platform's default
  "target": "C:\\ROMs\\snes\\Chrono Trigger.sfc",
  "available": true,             // false when a rescan cannot find the file
  "launchCount": 0,
  "createdAt": 0,
  "media": {                     // one slot per art type
    "cover":   { "path": "media\\<id>\\cover.png",   "source": "scraper:screenscraper" },
    "banner":  { "path": "media\\<id>\\banner.png",  "source": "user" },
    "logo":    null,
    "video":   null,
    "screenshot": null
  },
  "meta": { "developer": "…", "year": 1995, "genres": ["RPG"], "description": "…", "scrapedFrom": "…", "scrapedAt": "…" },
  "playtime": { "lastLaunched": "…", "seconds": 0 }
}
```

Other new storage:

- `profiles.json`: **player and emulator profiles**. Each profile has `id`, `name`, `kind` (`player` or `emulator`), `exe`, `args` as an **array of strings** with placeholders (`{file}`, `{dir}`, `{name}`) substituted per element, `platforms` it serves. A top-level `defaults` map picks one profile per platform id (`videos` for the video library), so there is never more than one default. An array, not one string, so no quoting or splitting is ever needed. Separate file so it is easy to share.
- `libraries` in `settings.json`: the library folders (path, type, include subfolders, platform overrides per subfolder).
- `media\<id>\`: user-chosen or scraped art, named by slot (`cover.jpg`, `banner.jpg`, `logo.png`). Kept out of `icons\`. Ids are UUIDs, checked with the same `UUID_RE` as backup import, so a crafted id can never write outside the folder.
- `scrape-cache\`: raw scraper responses, keyed by source and source id.
- `secrets.json`: scraper credentials, encrypted (§4).

Every JSON file is written with `writeFileAtomic`. **Backups:** `backup:export` adds `library` and `profiles` to the bundle (as `version: 3`; import keeps accepting 1 and 2). Art is left out because it can be large and can be fetched again; a later "Export with art" can write a zip. Secrets are never exported.

### 5.2 Main process modules (new)

| Module | Responsibility |
|---|---|
| `lib/media/library.js` | The `library.json` store (§5.1): load, save, normalize, backup and recovery |
| `lib/media/art-protocol.js` | Registers the `lpmedia:` protocol that serves art to the Media Mode window (§5.4) |
| `lib/media/players.js` | Player and emulator profiles: detect installed players, build argv from a template, validate paths |
| `lib/media/scan.js` | Scan a folder for video files (by extension) or ROMs (by platform extension list), and create entries |
| `lib/media/launch.js` | Launch a typed action; spawn with an **argv array**, never a shell string, and track the process so Media Mode can show "running" and return focus |
| `lib/media/art.js` | Read and write per-entry art; resize with `nativeImage.resize` and save as JPEG/PNG (no new dependency); manage the scraper-vs-user source flag |
| `lib/scrapers/*.js` | One module per source (§4); common queue and cache in `lib/scrapers/index.js` |
| `lib/scrapers/queue.js` | Per-source rate limit, concurrency, retry and backoff |

### 5.3 Launching

- **Video and ROMs:** `type: 'profile'`. The profile's `exe` plus its `args` array, with `{file}` replaced in each element, go straight to `spawn(exe, args, { shell: false })`. Example: `mpv.exe`, `["--fs", "{file}"]`; `retroarch.exe`, `["-L", "cores\\snes9x_libretro.dll", "-f", "{file}"]`.
- **Do not allow `.bat`/`.cmd` as a profile `exe`.** `detachedSpawn` has to switch to `shell: true` for those (Node refuses them otherwise), and then a ROM name with `&` or `%` is interpreted by `cmd.exe`. The profile editor rejects them; scripted setups can keep using normal Launchpad items.
- **Native PC games:** the existing `path` / `url` / `script` / `command` types, through the existing `launcher.launch`. Steam/Epic URIs (`steam://rungameid/…`) already work through `openExternal`.
- **Process tracking:** `lib/media/launch.js` keeps the child (not `detached` + `unref` like `detachedSpawn`) and listens for `exit`. For launchers that hand off to another process (Steam, some emulators), the child exits at once; treat that case as "launched, not tracked", and say so in the UI rather than guessing.
- **Focus:** on exit, bring the Media Mode window back with `win.show()` and `win.focus()`. The window-manager helper already has a `focus` command (`lib/windowmgr.js`) if Windows refuses the foreground change.
- **Launch counts** are recorded the same way `launchAndRecord` does for grid items, so "Continue" can sort by last played.

### 5.4 Renderer: Media Mode UI

A new window, `renderer/media/` (`index.html`, `media.js`, `media.css`), built like the existing panels, with its own preload (`media-preload.js`) that exposes `window.mediaAPI`. Add it to `build.files`.

- **Layout:** a horizontal carousel per library or category ("Continue", "Recently added", "Platforms", "Videos"). Large portrait covers for games, landscape banners or stills for video. The selected tile shows the banner as a background, the logo, and the description.
- **Input:** Controller, keyboard and mouse all work on every screen. Controller behavior, mapping and focus rules are in §5.6.
- **Modes:** "Games", "Videos", and "All". Filters and search work in every mode.
- **Editing:** a "Customize" sheet for any entry: pick a file for each art slot, paste a URL, pick from SteamGridDB results, or reset to the scraped image. Drag-and-drop works here too.
- **Theme:** reuse the existing theme tokens (accent, light/dark) so Media Mode matches the rest of Launchpad.
- **Art delivery:** every current window's CSP is `img-src 'self' data:`, and icons reach the renderer as data URLs. That does not scale to hundreds of covers. Register a privileged `lpmedia:` scheme with `protocol.handle` that serves only `media\<uuid>\<slot>.<ext>` (it checks the id against `UUID_RE` and the slot against a fixed list, so no other file is reachable). The Media Mode page's CSP is `img-src 'self' lpmedia:`. Tiles load lazily as they scroll in.
- **Window:** a separate `BrowserWindow` with the same `contextIsolation`, `sandbox` and no `nodeIntegration` as the main window. `media-preload.js` must be added to `build.files` in `package.json`, which lists each preload by name (`renderer/**/*` and `lib/**/*` are already covered).

Entry point: a button in the existing top bar ("Media Mode") and a hotkey in Settings. Esc or the Back button returns to the normal grid.

### 5.5 Library import and scanning

- **Add library folder:** choose a folder, choose its type (games with platform, or videos), and optionally a default player/emulator. Launchpad scans it and creates entries.
- **Platform detection:** from the folder name (`snes`, `psx`, `Nintendo 64`), from an extension list (`.sfc`, `.iso`, `.chd`), or from an explicit user pick. Keep a small built-in alias table, and let the user fix mismatches.
- **Video:** match by extension and read sidecar `.nfo` files and filename patterns (`Title (Year).mkv`).
- **Rescans** are incremental: new files are added, and missing files are marked "unavailable" rather than removed, so art and playtime are kept.

### 5.6 Controller support

Controller input is a first-class part of Media Mode, not an add-on. Every screen must be fully usable with a pad, and the keyboard and mouse stay as fallbacks.

**Input source.** Read pads in the renderer with the browser Gamepad API (`navigator.getGamepads()`, polled on `requestAnimationFrame`, with `gamepadconnected` and `gamepaddisconnected` for hot-plug). Chromium exposes XInput controllers (Xbox and most modern pads) with the standard mapping. Other pads (DirectInput-only, arcade sticks, many retro-style USB pads) show up with an unmapped layout, so we need a remap table for them. Verify this on real hardware in Phase 1.

**Focus model.**
- One focused element at a time, shown by a visible ring. The carousel tracks a focus index; no mouse hover changes focus while a pad is in use.
- D-pad and left stick move focus; within a row, left/right. Up/down switch rows and keep the closest column.
- Focus moves are edge-triggered (on press, not held) with a repeat after about 400 ms, then about 80 ms while held.
- The first input after a pad is connected or idle switches the UI to "pad mode": the button-prompt bar changes to match. Any keyboard or mouse input switches back.

**Default mapping (Xbox layout, the standard mapping).**

| Action | Button |
|---|---|
| Move focus | D-pad, left stick |
| Select / launch | A |
| Back / close | B |
| Customize art (on the focused item) | Y |
| Previous / next library tab | LB / RB |
| Menu (Media Mode settings, scraper) | Start |
| Quick actions (remove from library, rescrape) | X |
| Page up / down in long lists | LT / RT, or right stick |

**Remappable actions.** Every action above is listed in the Controller setup screen. The user can rebind each action to a button, reset to defaults, and tune the stick dead zone (default 35%). The table includes a conflict check: binding two actions to one button asks the user which one to keep.

**Storage.** The mapping lives in `settings.json` under `controller`: a button-to-action map keyed by the pad's id (vendor and product), plus the global dead zone. Keep one profile per pad so a second controller does not need remapping. Mapping data never goes through the renderer's persistence without the safe-file helper.

**Button prompts.** The bottom bar always shows the glyphs for the active layout (Xbox glyphs by default). Use one icon set for all screens, and keep the labels next to the glyphs so the bar is readable from a couch.

**Limits to plan for.**
- The Guide (Xbox) button is not exposed to web pages. Do not make a core action depend on it. Use Start or the Launchpad hotkey instead.
- Pads need the Media Mode window focused. If another app has focus, the hotkey (Ctrl+Space by default) is what brings Launchpad back.
- Launched emulators and players take their own input. Chromium only reports pad input to a focused page, so Media Mode stops reacting as soon as the game has focus. Still, ignore input while the Game running screen is up and for about 500 ms after the window gets focus back, so the button that closed the game does not also launch something. Many emulators also read pads directly, so the user may need to set their own controller mapping inside the emulator. The Launch profile can document this.
- Rumble and analogue triggers beyond the basic values are optional. Skip them in v1.

**Screens that carry controller design.** Home (games), Videos, Scraper review, Customize art and Controller setup each show the bar for the layout in use. See the Media Mode design artifact: https://claude.ai/artifact/8x1EMf45s551xxEnxkUEz5

**Implementation sketch.**
- `renderer/media/input/pad.js`: polls the Gamepad API, applies the dead zone, applies the mapping, emits named actions (`up`, `down`, `select`, `back`, `tabPrev`, `tabNext`, `customize`, `menu`).
- `renderer/media/input/focus.js`: the focus model, shared with keyboard and mouse handling so every screen uses one code path.
- `renderer/media/input/prompts.js`: builds the button-prompt bar from the active mapping.
- Main process: only persists the mapping (`controller` in `settings.json`). Pad reads never leave the renderer.

## 6. Scraping flow (ES-DE style)

1. **Choose sources** per field. For example: SteamGridDB for banners, ScreenScraper for covers and descriptions, IGDB for PC game metadata. Each source's credentials are entered once in Settings.
2. **Pick scope:** a library, a platform, selected entries, or only entries without art.
3. **Match:** search by cleaned filename, then fall back to a name search. Score each candidate (title similarity, platform, year). Auto-accept high scores; send low scores to a review list.
4. **Review:** a list where the user can accept, pick another result, or skip. Scrapes can run unattended, but the review list is always available.
5. **Fetch media:** download only the art types the user checked. Resize and store under `media\<id>\`.
6. **Write:** update the entry's `meta` and `media` slots, marking the source. Never overwrite a slot whose source is `user` unless the user chose "replace".
7. **Resume:** the scrape queue and cache are on disk, so an interrupted run restarts where it stopped.

Legal and terms note: ScreenScraper, TheGamesDB, IGDB and SteamGridDB all have terms of service, attribution and quota rules. The implementation must follow them, and must not bundle any developer keys in the repository. Users enter their own keys.

## 7. Phased plan

**Phase 0: Decisions** (settled as defaults; change them before Phase 1 if they are wrong)
- Windows only, as now. Controller support is in v1. Video metadata (TMDb/NFO) waits for Phase 4; Phase 1 videos show the file name.
- First scrapers: **SteamGridDB** (simple key, strong art) and **TheGamesDB** (simple metadata). ScreenScraper and IGDB follow in Phase 3.

**Phase 1: Launch and library, no scraping.** Suggested order, one PR each:
1. `lib/media/library.js` store with tests, modeled on `test/store.test.js` (load, save, `.bak` recovery, read-only mode, UUID ids).
2. `lib/media/players.js` and `profiles.json`: argv building from the template, `.bat`/`.cmd` rejection, presets for mpv, VLC and RetroArch. Tests for placeholder substitution with spaces, quotes, `&` and `%` in file names.
3. `lib/media/scan.js`: folder scan, extension and folder-name platform detection, incremental rescan with `available: false`. Tests on a temp folder tree.
4. `lib/media/launch.js` and the IPC for it: tracked spawn, exit event to the renderer, launch counts.
5. Media Mode window: `media-preload.js` (added to `build.files`), `renderer/media/`, entry button and hotkey. Home, Videos, Add library folder, Players and emulators, and Game running screens from the design canvas.
6. Controller input: `pad.js`, `focus.js`, `prompts.js`, default mapping and dead zone (§5.6). Remapping waits for Phase 2.
7. Update `CODEBASE_SUMMARY.md` (new files, IPC channels, storage) and `README.md` (Your data table).
- **Done when:** a user can add a folder of `.mkv` files and a folder of `.sfc` ROMs, choose mpv and RetroArch, and launch from Media Mode with a controller, and `npm test` passes.

**Phase 2: Art and customization**
- `media\<id>\` storage, the `lpmedia:` protocol, per-slot set/clear, drag-and-drop, URL import (main process, reusing `icons.js`'s capped fetch, with size and type checks).
- Backup format `version: 3` with library and profiles.
- Customize sheet and the source flag.
- Controller remapping screen, per-pad profiles, and the conflict check (§5.6).
- **Done when:** every slot can be set by hand and survives a rescan and an export/import round trip.

**Phase 3: Scrapers**
- `lib/scrapers/` interface, shared queue and cache.
- SteamGridDB and TheGamesDB first, then ScreenScraper, then IGDB.
- Match, review list, resumable batch.
- **Done when:** a 200-ROM folder can be scraped with covers and descriptions, and a re-run makes no new requests for cached items.

**Phase 4: Polish**
- Playtime tracking and "Continue" row.
- Video metadata (NFO, TMDb/OMDb) and season or episode grouping.
- Per-platform emulator defaults and per-game overrides.
- Controller-first polish (button prompts, on-screen keyboard for search).

## 8. Risks and open questions

- **Process tracking is imperfect.** Many emulators and launchers hand off to another process. Keep the "running" state approximate and document it.
- **Credentials.** Scraper keys must stay in main. IGDB's client secret cannot be shipped in the app or in a renderer. Confirm each provider's terms for desktop apps that store user keys.
- **Rate limits and quotas.** ScreenScraper's quota model and developer-key rules were not confirmed in this research. Check them before phase 3.
- **Emulator command lines differ.** Each emulator needs its own profile. Ship a few well-tested presets (mpv, VLC, RetroArch) and leave the rest to the user.
- **Gamepad in Electron.** The Gamepad API works in Chromium, but the window must have focus for input, the Guide button is not exposed, and non-XInput pads need a remap. Test with the actual controllers the user owns, and keep the keyboard fallback (§5.6).
- **Size of art.** Scraped media can be large. Store resized copies, and let the user set a cache limit.
- **Old data.** `apps.json` is not touched, so existing items keep working with no migration. Old backups (version 1 and 2) must still import.
- **Scope creep.** Video metadata, playtime, and emulator-specific save handling are each a project. Keep them out of phase 1.

## 9. Readiness review (2026-10-22)

The plan was checked against `lib/store.js`, `lib/launcher.js`, `lib/settings.js`, `lib/icons.js`, `lib/safe-file.js`, the backup and launch handlers in `main.js`, the windows' CSP, and `package.json`. It is ready for Phase 1 with these changes, which are already written into the sections above:

| Was | Problem found | Now |
|---|---|---|
| Library entries in `apps.json` | Every entry gets a Home grid cell, is sent with a data-URL icon on every `apps:list`, and is base64-embedded in backups | Separate `library.json` store (§5.1) |
| New field names (`title`, `launch.type`) | `apps.json` already uses `name`, `type`, `target`, `args` | Same names; one new `type: 'profile'` |
| Args as a template string | `launcher.js` splits strings with a regex, and `.bat`/`.cmd` force `shell: true` | Args as an array; `.bat`/`.cmd` rejected as profile programs (§5.3) |
| Reuse the launcher for tracking | `detachedSpawn` unrefs the child and returns nothing | New tracked spawn in `lib/media/launch.js` |
| Art as data URLs like icons | CSP allows only `'self'` and `data:`; data URLs do not scale to hundreds of covers | `lpmedia:` protocol limited to the media folder (§5.4) |
| Credentials in `settings.json` | Would sit in plain text beside other settings | `secrets.json` with `safeStorage` (§4) |
| Art in backups | One JSON file; covers would make it huge | Art left out; library and profiles in (§5.1) |
| New preload "just works" | `build.files` lists each preload by name | Add `media-preload.js` there (§5.4) |
| Image resizing | Not specified; would tempt a native dependency | `nativeImage.resize`, no new dependency |

Still open, and not blocking Phase 1:
- ScreenScraper developer-key and quota rules, and IGDB limits (Phase 3).
- Behavior of non-XInput pads, checked on the user's own controllers during Phase 1 step 6.
- Whether Media Mode should later replace the main window in fullscreen instead of being a second window. Phase 1 uses a second window because it keeps the existing grid code untouched.

## 10. Sources

- ES-DE and RetroDECK scraping documentation: https://retrodeck.readthedocs.io/en/latest/wiki_management/about-scraping/ (ScreenScraper and TheGamesDB behavior, scrape filters)
- Batocera scraper documentation: https://www.wiki.batocera.org/_export/raw/scrape_from (IGDB as a scraper source)
- Playnite comparison and feature overview: https://api.playnite.link/docs/manual/gettingStarted/comparisonWithOtherSoftware.html (emulator profiles, metadata add-ons, plugin and script extensibility)
- IGDB authentication, Twitch forum: https://discuss.dev.twitch.com/t/igdb-authentication-and-tokens-need-server-app/28394 (client-credentials tokens; secret must stay server-side)

The IGDB rate figures and the ScreenScraper quota model came from secondary sources. Verify them against api-docs.igdb.com and the ScreenScraper site before implementing.
