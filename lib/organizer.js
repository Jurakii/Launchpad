// Snaps other apps' windows into layout zones: from the taskbar's "Snap to"
// menu, from the organizer panel along the top of each display (lib/panel.js),
// or by dragging a window to the top of a display and dropping it on a zone
// in the strip that appears there. The grid placer (lib/grid.js), opened
// with a hotkey or the panel's grid button, sizes a window to any block of
// cells instead. Zones are worked out in DIP on the Electron side and
// converted to physical pixels just before the helper moves the window, so
// mixed-DPI monitors line up.
const path = require('path');
const { BrowserWindow, Notification, globalShortcut, screen } = require('electron');
const layouts = require('./layouts');
const zones = require('./zones');
const strip = require('./strip');
const icons = require('./icons');
const panel = require('./panel');
const gridWindows = require('./grid');
const windowmgr = require('./windowmgr');

const ERROR_MESSAGES = {
  gone: 'That window has closed.',
  denied: "Windows won't let Launchpad move this window because the app is running as administrator.",
};
const GENERIC_ERROR = "Launchpad couldn't move that window.";

// Windows timers tick every ~15.6 ms, so a 16 ms interval actually runs
// every other tick (~32 Hz, measured). Just under one tick gives ~64 Hz.
const CURSOR_POLL_MS = 10;

const OVERLAY_HTML = path.join(__dirname, '..', 'renderer', 'overlay.html');
const OVERLAY_PRELOAD = path.join(__dirname, '..', 'overlay-preload.js');

const NOTHING_TO_MOVE = 'Click the window you want to move first.';

const failure = (reply) => ({ ok: false, error: ERROR_MESSAGES[reply && reply.code] || GENERIC_ERROR });

// The user is in another app, so Launchpad's own dialogs wouldn't be seen.
function notify(message) {
  if (Notification.isSupported()) new Notification({ title: 'Launchpad', body: message }).show();
}

function findZone(all, layoutId, zoneId) {
  const layout = all.find((l) => l.id === layoutId);
  const zone = layout && layout.zones.find((z) => z.id === zoneId);
  return zone ? { layout, zone } : null;
}

// The Electron display for a monitor rect from the helper ([l, t, r, b] in
// physical pixels). Matched exactly when possible; the nearest display to
// its centre covers any rounding differences.
function displayForMonitor([left, top, right, bottom]) {
  const exact = screen.getAllDisplays().find((d) => {
    const p = screen.dipToScreenRect(null, d.bounds);
    return p.x === left && p.y === top && p.x + p.width === right && p.y + p.height === bottom;
  });
  return exact || screen.getDisplayNearestPoint(screen.screenToDipPoint({ x: (left + right) / 2, y: (top + bottom) / 2 }));
}

async function placeInZone(handle, layout, zone, display, gap) {
  const target = zones.zoneRect(layout, zone, display.workArea, gap);
  const placed = await windowmgr.place(handle, screen.dipToScreenRect(null, target));
  return placed.ok ? { ok: true } : failure(placed);
}

// From the "Snap to" menu: moves the window into a zone on the monitor it's
// already on, then brings it to the front so the result is visible over
// Launchpad. Resolves to { ok } or { ok: false, error } with a message ready
// to show the user.
async function snapWindow(handle, layoutId, zoneId) {
  const { layouts: all, gap } = layouts.load();
  const found = findZone(all, layoutId, zoneId);
  if (!found) return { ok: false, error: 'That layout no longer exists.' };

  const monitor = await windowmgr.monitorOf(handle);
  if (!monitor.ok) return failure(monitor);
  const result = await placeInZone(handle, found.layout, found.zone, displayForMonitor(monitor.bounds), gap);
  if (result.ok) await windowmgr.focus(handle); // best effort: Windows can refuse to change the foreground window
  return result;
}

// ---------- settings ----------

let enabled = false;
let dragSnapping = false;
let watchingDisplays = false;

windowmgr.onEvent((ev) => {
  if (ev.ev === 'dragStart') startDrag(ev);
  else if (ev.ev === 'moveEnd') finishDrag(ev);
  else if (ev.ev === 'foreground') setFocused(ev);
  else if (ev.ev === 'ready') setHelperUp(true);
  else if (ev.ev === 'helperExit') {
    cancelDrag();
    setHelperUp(false);
  }
});

// { enabled, panel: 'always' | 'autoHide' | 'off', snapOnDrag, gridHotkey },
// from settings.organizerOptions(). enabled is the master switch: while it's
// off, only the taskbar's "Snap to" menu works, and the organizer doesn't
// keep the helper running (the hotkey setting may still).
function configure(options) {
  enabled = !!options.enabled;
  if (enabled) windowmgr.setKeepAlive('organizer', true);
  dragSnapping = enabled && !!options.snapOnDrag;
  if (!dragSnapping) cancelDrag();
  if (enabled) {
    if (!registerGridHotkey(options.gridHotkey)) console.error(`Could not register the grid hotkey "${options.gridHotkey}".`);
  } else {
    unregisterGridHotkey();
    closeGrid(false);
    gridWindows.destroyAll();
    destroyOverlays();
  }
  // Overlays and grids are sized to their display's work area, so any
  // display change throws them away; they're recreated when next needed.
  if (enabled && !watchingDisplays) {
    watchingDisplays = true;
    for (const event of ['display-added', 'display-removed', 'display-metrics-changed']) {
      screen.on(event, () => {
        cancelDrag();
        destroyOverlays();
        closeGrid(false);
        gridWindows.destroyAll();
      });
    }
  }
  loadPanelLayouts();
  panel.setMode(enabled ? options.panel : 'off');
  if (focused) setFocused(focused); // known from before, while the hotkey setting kept the helper running
  else pushPanelContent();
  // After the panels are gone, so the helper they asked to give back their
  // space is still there to do it.
  if (!enabled) windowmgr.setKeepAlive('organizer', false);
}

// ---------- the panel ----------
// The panel shows every layout, and clicking one of its zones moves the
// focused window there, on the panel's display. The panel never takes focus
// itself (see lib/panel.js), so "the focused window" is still the one the
// user was working in when they clicked.

let panelSource = { layouts: [], gap: 0 }; // the layouts it shows, as of the last change
let focused = null; // the latest foreground event
let focusedIcon = null; // { exe, dataUrl }
let iconFetching = null; // exe; titles can change many times before it's back
let helperUp = false;

function loadPanelLayouts() {
  const data = layouts.load();
  panelSource = { layouts: layouts.visible(data), gap: data.gap };
}

// A layout was added, changed, removed, hidden or shown, or the gap changed.
// The drag strip and the grid read layouts.json each time they open.
function layoutsChanged() {
  loadPanelLayouts();
  if (enabled) pushPanelContent();
}

// The window the panel shows and acts on. While the grid is open it has the
// focus itself, so that's the window it's moving.
const panelTarget = () => (grid ? grid.target : focused);

function setFocused(ev) {
  const switched = !focused || focused.hwnd !== ev.hwnd;
  focused = ev;
  if (!enabled) return;
  pushPanelContent();
  if (ev.exe && (!focusedIcon || focusedIcon.exe !== ev.exe)) fetchFocusedIcon(ev.exe);
  if (switched) panel.focusChanged();
}

function fetchFocusedIcon(exe) {
  if (iconFetching === exe) return;
  iconFetching = exe;
  icons.exeIconDataUrl(exe).then((dataUrl) => {
    if (iconFetching === exe) iconFetching = null;
    focusedIcon = { exe, dataUrl };
    if (focused && focused.exe === exe) pushPanelContent();
  });
}

function setHelperUp(up) {
  helperUp = up;
  if (!up) focused = null;
  if (enabled) pushPanelContent();
}

function iconFor(win) {
  return win && win.exe && focusedIcon && focusedIcon.exe === win.exe ? focusedIcon.dataUrl : null;
}

function pushPanelContent() {
  const target = panelTarget();
  const shown = target && target.visible && (target.kind === 'app' || target.kind === 'own');
  panel.setContent({
    layouts: panelSource.layouts,
    focused: shown ? { title: target.title, iconDataUrl: iconFor(target), snappable: !!target.snappable } : null,
    helperUp,
    gridHotkey,
  });
}

// A click on a zone in the panel that sender (a panel's webContents) belongs
// to. Resolves to { ok } or { ok: false, error } for the panel to show.
async function snapFromPanel(sender, layoutId, zoneId) {
  const display = panel.displayForSender(sender);
  if (!display) return { ok: false, error: 'Not a panel.' };
  const target = panelTarget();
  if (!target || !target.snappable) return { ok: false, error: NOTHING_TO_MOVE };
  const found = findZone(panelSource.layouts, layoutId, zoneId);
  if (!found) return { ok: false, error: 'That layout no longer exists.' };
  const result = await placeInZone(target.hwnd, found.layout, found.zone, display, panelSource.gap);
  if (result.ok && grid) closeGrid(true); // a zone picked in the panel instead of cells
  return result;
}

// The cursor moved onto a zone in a panel (zone = { layoutId, zoneId }) or
// off it (null): shows where the focused window would go, full size, on the
// overlay for that panel's display.
function previewFromPanel(sender, zone) {
  const display = panel.displayForSender(sender);
  if (!display || drag) return;
  const found = zone && findZone(panelSource.layouts, zone.layoutId, zone.zoneId);
  if (!found) {
    hideOverlay(display.id);
    return;
  }
  const wa = display.workArea;
  const r = zones.zoneRect(found.layout, found.zone, wa, panelSource.gap);
  showOverlay(display, { strip: null, hovered: zone, preview: { x: r.x - wa.x, y: r.y - wa.y, width: r.width, height: r.height } });
}

// ---------- the grid placer ----------
// The grid (lib/grid.js) opens for the focused window: from the hotkey, on
// the display that window is on, or from a panel's grid button, on that
// panel's display. It starts on the cells nearest where the window already
// is. The grid takes the focus while it's open, so the window it moves is
// remembered here, and gets the focus back when the grid closes.

// Cells across and down: 6 x 4, or 4 x 6 on a display taller than it's wide.
const GRID_LONG_SIDE = 6;
const GRID_SHORT_SIDE = 4;

let grid = null; // { target, display, cols, rows, gap, placing } while it's open
let gridOpening = false;
let gridHotkey = null; // registered while the organizer is on

gridWindows.onBlur((displayId) => {
  // The user clicked something else. Placing a maximized window restores
  // it, which takes the focus too, so that doesn't count.
  if (grid && grid.display.id === displayId && !grid.placing) closeGrid(false);
});

// display: where to open it, or null for the display the window is on.
// Asking again where it's already open closes it. Resolves to { ok } or
// { ok: false, error }.
async function openGrid(display) {
  if (grid && (!display || display.id === grid.display.id)) {
    closeGrid(true);
    return { ok: true };
  }
  if (gridOpening) return { ok: true };
  // From another display's panel while it's open: the same window, there.
  const target = grid ? grid.target : focused && focused.snappable ? focused : null;
  if (!target) return { ok: false, error: NOTHING_TO_MOVE };
  gridOpening = true;
  let monitor;
  try {
    monitor = await windowmgr.monitorOf(target.hwnd);
  } finally {
    gridOpening = false;
  }
  if (!monitor.ok) return failure(monitor);

  const d = display || displayForMonitor(monitor.bounds);
  const wa = d.workArea;
  const [l, t, r, b] = monitor.frame;
  const frame = monitor.minimized ? null : screen.screenToDipRect(null, { x: l, y: t, width: r - l, height: b - t });
  const portrait = wa.height > wa.width;
  const cols = portrait ? GRID_SHORT_SIDE : GRID_LONG_SIDE;
  const rows = portrait ? GRID_LONG_SIDE : GRID_SHORT_SIDE;
  const { gap } = layouts.load();
  grid = { target, display: d, cols, rows, gap, placing: false };
  gridWindows.show(d, {
    cols,
    rows,
    gap,
    area: { width: wa.width, height: wa.height },
    frame: frame && { x: frame.x - wa.x, y: frame.y - wa.y, width: frame.width, height: frame.height },
    title: target.title,
    iconDataUrl: iconFor(target),
  });
  pushPanelContent();
  return { ok: true };
}

function openGridFromPanel(sender) {
  const display = panel.displayForSender(sender);
  if (!display) return Promise.resolve({ ok: false, error: 'Not a panel.' });
  return openGrid(display);
}

// The cells picked in the grid (sender: its webContents), as { x, y, w, h }.
// Resolves to { ok } or { ok: false, error } for the grid to show. The grid
// closes once the window is in place.
async function placeFromGrid(sender, block) {
  if (!grid || !gridWindows.isFrom(sender)) return { ok: false, error: 'The grid has closed.' };
  const g = grid;
  const b = block || {};
  const cells = { id: 'grid', name: 'Grid', cols: g.cols, rows: g.rows, zones: [{ id: 'cells', x: b.x, y: b.y, w: b.w, h: b.h }] };
  if (zones.validateLayout(cells)) return { ok: false, error: 'Pick the cells to fill first.' };
  g.placing = true;
  let result;
  try {
    result = await placeInZone(g.target.hwnd, cells, cells.zones[0], g.display, g.gap);
  } catch (err) {
    result = { ok: false, error: err.message };
  }
  g.placing = false;
  if (result.ok && grid === g) closeGrid(true);
  return result;
}

function cancelGrid(sender) {
  if (gridWindows.isFrom(sender)) closeGrid(true);
}

// refocus: give the window the focus back (Esc, or after placing it), but
// not when the grid closed because the user clicked something else.
async function closeGrid(refocus) {
  if (!grid) return;
  const { target } = grid;
  grid = null;
  // Before the grid hides, while Launchpad is still in front: Windows only
  // lets the foreground process (or one it started, like the helper) bring
  // another window to the front.
  if (refocus) await windowmgr.focus(target.hwnd).catch(() => {});
  if (!grid) gridWindows.hide(); // unless it was opened again meanwhile
  pushPanelContent();
}

function onGridHotkey() {
  openGrid(null).then(
    (result) => {
      if (!result.ok) notify(result.error);
    },
    (err) => notify(err.message)
  );
}

// Swaps the registered accelerator for accel, keeping the old one working
// if accel can't be had (another app has it). Returns whether it was.
function registerGridHotkey(accel) {
  if (accel === gridHotkey) return true;
  const previous = gridHotkey;
  unregisterGridHotkey();
  let ok = false;
  try {
    ok = !!accel && globalShortcut.register(accel, onGridHotkey);
  } catch {
    // not an accelerator Electron understands
  }
  if (ok) gridHotkey = accel;
  else if (previous && globalShortcut.register(previous, onGridHotkey)) gridHotkey = previous;
  return ok;
}

function unregisterGridHotkey() {
  if (!gridHotkey) return;
  try {
    globalShortcut.unregister(gridHotkey);
  } catch {
    // ignore
  }
  gridHotkey = null;
}

// From Settings. While the organizer is off nothing is registered; the new
// keys are tried when it's turned on.
function setGridHotkey(accel) {
  if (!enabled) return { ok: true };
  if (!registerGridHotkey(accel)) return { ok: false, error: `"${accel}" is already in use by another app.` };
  pushPanelContent(); // its grid button names the keys
  return { ok: true };
}

// The grid's registered accelerator, or null.
const currentGridHotkey = () => gridHotkey;

// ---------- drag to snap ----------
// The helper reports when a window's move loop starts and ends. In between,
// main polls the cursor (the dragged window owns mouse capture, so nothing
// of ours receives mouse events) and drives one transparent, click-through
// overlay per display that draws the strip and the hovered zone. The strip
// hangs from the top of the work area, so with the panel docked it drops
// out of the panel's bottom edge.

let drag = null; // { hwnd, layouts, gap, timer, display, strip, showing, hovered }
const overlays = new Map(); // display id -> { win, ready, state }

// The helper only sends dragStart for a window being moved (not resized)
// that's worth snapping.
function startDrag(ev) {
  cancelDrag();
  if (!dragSnapping) return;
  for (const id of overlays.keys()) hideOverlay(id); // a preview from the panel, if any
  panel.setDragging(true);
  const data = layouts.load();
  drag = { hwnd: ev.hwnd, layouts: layouts.visible(data), gap: data.gap, timer: null, display: null, strip: null, showing: false, hovered: null };
  drag.timer = setInterval(trackCursor, CURSOR_POLL_MS);
  trackCursor();
}

function trackCursor() {
  const p = screen.getCursorScreenPoint();
  const display = screen.getDisplayNearestPoint(p);
  if (!drag.display || display.id !== drag.display.id) {
    if (drag.display) hideOverlay(drag.display.id);
    drag.display = display;
    drag.strip = strip.computeStrip(drag.layouts, display.workArea);
    drag.showing = false;
    drag.hovered = null;
  }
  const showing = strip.shouldShow(drag.strip, display.workArea, p, drag.showing);
  const hovered = showing ? strip.zoneAt(drag.strip, p) : null;
  const sameZone = hovered && drag.hovered
    ? hovered.layoutId === drag.hovered.layoutId && hovered.zoneId === drag.hovered.zoneId
    : hovered === drag.hovered;
  if (showing === drag.showing && sameZone) return;
  drag.showing = showing;
  drag.hovered = hovered;
  if (showing) showOverlay(display, overlayState());
  else hideOverlay(display.id);
}

// What the overlay draws, relative to its window (which covers the work area).
function overlayState() {
  const wa = drag.display.workArea;
  const rel = (r) => ({ x: r.x - wa.x, y: r.y - wa.y, width: r.width, height: r.height });
  const found = drag.hovered && findZone(drag.layouts, drag.hovered.layoutId, drag.hovered.zoneId);
  return {
    strip: {
      rect: rel(drag.strip.rect),
      items: drag.strip.items.map((item) => ({
        layoutId: item.layoutId,
        name: item.name,
        thumb: rel(item.thumb),
        zones: item.zones.map((z) => ({ zoneId: z.zoneId, rect: rel(z.rect) })),
      })),
    },
    hovered: drag.hovered,
    preview: found ? rel(zones.zoneRect(found.layout, found.zone, wa, drag.gap)) : null,
  };
}

async function finishDrag(ev) {
  if (!drag || ev.hwnd !== drag.hwnd) return;
  trackCursor(); // the last poll can be up to one interval old
  const { hwnd, hovered, display, layouts: all, gap } = drag;
  cancelDrag();
  // Esc, or let go anywhere but a zone: Windows' own handling stands.
  if (ev.cancelled || !hovered) return;
  const found = findZone(all, hovered.layoutId, hovered.zoneId);
  let result;
  try {
    result = await placeInZone(hwnd, found.layout, found.zone, display, gap);
  } catch (err) {
    result = { ok: false, error: err.message };
  }
  if (!result.ok) notify(result.error);
}

function cancelDrag() {
  if (!drag) return;
  clearInterval(drag.timer);
  if (drag.display) hideOverlay(drag.display.id);
  drag = null;
  panel.setDragging(false);
}

function overlayFor(display) {
  const existing = overlays.get(display.id);
  if (existing && !existing.win.isDestroyed()) return existing;
  const win = new BrowserWindow({
    ...display.workArea,
    show: false,
    frame: false,
    transparent: true,
    backgroundColor: '#00000000',
    hasShadow: false,
    focusable: false, // never steal focus from the window being dragged
    skipTaskbar: true,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    webPreferences: {
      preload: OVERLAY_PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      backgroundThrottling: false, // it's hidden between drags but must draw at once when shown
    },
  });
  win.setIgnoreMouseEvents(true);
  win.setAlwaysOnTop(true, 'screen-saver');
  const overlay = { win, ready: false, state: null };
  win.webContents.once('did-finish-load', () => {
    overlay.ready = true;
    if (overlay.state) win.webContents.send('overlay:state', overlay.state);
  });
  win.loadFile(OVERLAY_HTML);
  overlays.set(display.id, overlay);
  return overlay;
}

function showOverlay(display, state) {
  const overlay = overlayFor(display);
  overlay.state = state;
  if (overlay.ready) overlay.win.webContents.send('overlay:state', state);
  if (!overlay.win.isVisible()) overlay.win.showInactive();
}

function hideOverlay(displayId) {
  const overlay = overlays.get(displayId);
  if (!overlay || overlay.win.isDestroyed()) return;
  overlay.state = null;
  // Cleared before hiding so the next show doesn't flash the old strip.
  if (overlay.ready) overlay.win.webContents.send('overlay:state', null);
  overlay.win.hide();
}

function destroyOverlays() {
  for (const { win } of overlays.values()) {
    if (!win.isDestroyed()) win.destroy();
  }
  overlays.clear();
}

module.exports = {
  snapWindow,
  configure,
  layoutsChanged,
  snapFromPanel,
  previewFromPanel,
  openGridFromPanel,
  placeFromGrid,
  cancelGrid,
  setGridHotkey,
  currentGridHotkey,
};
