# Organizer Phase 0 spikes

Throwaway code that tests the risky Win32 parts of the top bar window organizer
before any real feature code is written. Nothing here ships: `spikes/` is not in
`build.files`, and `npm test` doesn't look here.

| File | What it is |
|---|---|
| `windowmgr-spike.ps1` | Prototype of the long-lived helper: inline C# with a WinEvent hook on its own message-loop thread, plus JSON-lines commands on stdin (`place`, `describe`, `find`, `monitors`, `show`, `close`, `probe`, `appbarNew`, `appbarSetPos`, `appbarRemove`). |
| `harness.js` | Electron main script that drives the helper and prints PASS/FAIL/INFO lines. Writes `last-run.json` (git-ignored). |

## Running it

```
node_modules\electron\dist\electron.exe spikes\organizer\harness.js
node_modules\electron\dist\electron.exe spikes\organizer\harness.js --watch 120
```

The first runs the automated checks. `--watch N` then spends N seconds snapping
Notepad into the left half of whichever monitor you drag it to, and logs every
drag. While it runs, a 32px test bar reserves the top of the primary monitor,
a fullscreen window flashes for about 2 s, and a Notepad window opens (it's
sent WM_CLOSE at the end).

## Results (2026-10-13, Windows 10 19045, Electron 44.2.0)

Setup: primary 2560×1440 at 100%, second monitor 1920×1080 at 100% to its left.
Both had a work area equal to the full screen (no visible Windows taskbar).

| Question | Result |
|---|---|
| Helper start time (spawn + `Add-Type` compile) | 270–290 ms |
| Command round trip over stdin/stdout | median under 1 ms, worst 12 ms |
| WinEvent hook inside PowerShell, own message loop | Works. Move start/end reach the hook in under one 15.6 ms tick, and Node 0–1 ms later. |
| Foreground (focus) events from the same hook | 8 recorded, median 0 ms in the hook and 0 ms to Node. This can replace `foreground.ps1`'s 200 ms polling. |
| Telling a move from a resize | `WM_NCHITTEST` at move start: 2 (`HTCAPTION`) for title-bar drags, 11 (`HTRIGHT`) for a right-border resize. Comparing sizes at the end is **not** reliable: Aero Snap changes the size during a move. |
| Exact placement with invisible borders | 0 px error on both monitors, 4–10 ms per placement including restore. Notepad's invisible borders at 100% are `[7, 0, 7, 7]` (left, top, right, bottom). Moves at the same DPI needed one pass. |
| `dipToScreenRect` vs the helper's physical monitor rects | Match on both monitors (at 100% this is a weak test, see below). |
| AppBar registered **from the helper** on Electron's HWND | Works. Work area shrinks by 32 px, Electron fires `display-metrics-changed`, a maximized window stops below the bar, `ABM_REMOVE` gives the space back. |
| `hookWindowMessage` gets AppBar callbacks | Yes: `ABN_FULLSCREENAPP` with lParam 1 when a fullscreen window opens, 0 when it closes. |
| Aero Snap when a drag reaches the top edge | Windows maximizes the window **before** `MOVESIZEEND` arrives (`zoomed: true` at the end event), even with the AppBar covering the top 32 px. Restore-then-place at the end event works and is still in place 150 ms and 500 ms later. |
| Cursor polling in main with `setInterval(…, 16)` | Only 33–39 Hz, because Windows timer ticks are 15.6 ms. |
| UIPI probe (re-apply each window's own rect) | 3 windows probed, all movable, all elevation-readable. No elevated window was open, so the failure path is untested. |

The full numbers for the last run are in `last-run.json`.

## What this changes for Phase 1

- **Keep PowerShell + inline C#.** Startup and latency are well within budget,
  so there's no need for a native FFI dependency. The AppBar risk is retired.
- **The helper exits when stdin closes.** That's simpler than `foreground.ps1`'s
  parent-PID polling and should be the pattern for `lib/windowmgr.ps1`.
- **Use thread-level DPI awareness.** `SetThreadDpiAwarenessContext(-4)`
  works inside `powershell.exe`. Every thread that reads or sets coordinates
  needs it, including short-lived follow-up threads.
- **Set the helper's stdout to UTF-8.** The existing helpers don't, and they
  mangle non-ASCII text (see "Found along the way").
- **Decide move vs resize with `WM_NCHITTEST` at move start**, converting the
  point with `PhysicalToLogicalPointForPerMonitorDPI` first.
- **`place` must restore before positioning**, since Aero Snap may already have
  maximized the window by the time the end event arrives. In Phase 2, place
  only when a strip zone was hovered at release; a release at the very top
  with no zone should keep Windows' maximize.
- **Poll the cursor faster than 16 ms, or let the helper drive it.** 35 Hz is
  usable for highlighting zones but may feel laggy. Options for Phase 2: a
  shorter interval, or `EVENT_OBJECT_LOCATIONCHANGE` for the dragged window
  only, which is event-driven.

## Still open

1. **150% scaling.** Both monitors are at 100%, so the second `place` pass
   (for moves between different DPIs) never ran. To test: set one monitor to
   150% in Settings › System › Display, rerun the automated harness, and look
   for `"passes"` with two entries and `exact: true`. Then set it back.
2. **Admin windows (UIPI).** Open an elevated window (Task Manager on most
   admin accounts) and rerun. The probe should list it with `win32Error: 5`.
3. **Dragging onto the second monitor by hand.** Placement there is proven
   programmatically, but no drag onto it was recorded. Run with `--watch 60`
   and drag Notepad to the left monitor.
4. **Crash without `ABM_REMOVE`.** Not tested on purpose: if Explorer doesn't
   reclaim the space when the bar's window dies, the work area stays shrunk
   until Explorer restarts. Worth testing once in a VM or on a spare session.

## Found along the way

The existing helpers (`lib/windows.ps1` and the others) write JSON in the
console's OEM code page. A title like `Café — 日本` reaches Node as
`Caf� - ??`. This is separate from the organizer and is tracked as its own task.
