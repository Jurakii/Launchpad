// The grid placer's windows. The grid covers one display's work area with a
// grid of cells, and the user picks a block of them to size a window to,
// gTile-style: with the mouse, or with the arrow keys and Enter. There's
// one window per display, made the first time the grid opens there and
// kept, hidden, for next time; a display change throws them away, since
// each is sized to its display's work area. What the grid shows and what
// happens when cells are picked is up to lib/organizer.js; this file only
// manages the windows.
//
// Unlike the panel and the drag overlay, the grid takes the focus while it's
// open, because it needs the keyboard. The organizer picks the window to
// move before opening it, and hands the focus back when it closes.
const path = require('path');
const { BrowserWindow } = require('electron');

const GRID_HTML = path.join(__dirname, '..', 'renderer', 'grid.html');
const GRID_PRELOAD = path.join(__dirname, '..', 'grid-preload.js');

const windows = new Map(); // display id -> { win, ready, state }
let shownId = null; // the display the grid is open on
let blurListener = () => {};

function windowFor(display) {
  const existing = windows.get(display.id);
  if (existing && !existing.win.isDestroyed()) return existing;
  const win = new BrowserWindow({
    ...display.workArea,
    show: false,
    frame: false,
    transparent: true,
    backgroundColor: '#00000000',
    hasShadow: false,
    // A tool window: kept out of Alt+Tab, and passed over by the helper's
    // fullscreen check, which would otherwise take a grid covering a whole
    // display (no taskbar, panel auto-hiding) for a fullscreen app.
    type: 'toolbar',
    skipTaskbar: true,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    webPreferences: {
      preload: GRID_PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.setAlwaysOnTop(true, 'screen-saver'); // see createPanel in lib/panel.js
  const entry = { win, ready: false, state: null };
  win.webContents.once('did-finish-load', () => {
    entry.ready = true;
    if (shownId === display.id && entry.state) reveal(entry);
  });
  // Clicking another display or the taskbar, or switching apps.
  win.on('blur', () => {
    if (shownId === display.id) blurListener(display.id);
  });
  win.loadFile(GRID_HTML);
  windows.set(display.id, entry);
  return entry;
}

function reveal(entry) {
  entry.win.webContents.send('grid:state', entry.state);
  entry.win.show();
  entry.win.focus();
}

// Opens the grid on a display (closing it anywhere else) with what to show:
// { cols, rows, gap, area, frame, title, iconDataUrl }, see renderer/grid.js.
function show(display, state) {
  if (shownId !== null && shownId !== display.id) hide();
  const entry = windowFor(display);
  entry.state = state;
  shownId = display.id;
  if (entry.ready) reveal(entry); // otherwise it's shown once it has loaded
}

function hide() {
  const entry = shownId !== null ? windows.get(shownId) : null;
  shownId = null; // first, so hiding doesn't report a blur
  if (!entry || entry.win.isDestroyed()) return;
  entry.state = null;
  entry.win.hide();
  // Cleared so the next show doesn't flash this one's selection.
  if (entry.ready) entry.win.webContents.send('grid:state', null);
}

// Whether a message came from the grid that's open.
function isFrom(sender) {
  const entry = shownId !== null ? windows.get(shownId) : null;
  return !!entry && !entry.win.isDestroyed() && entry.win.webContents === sender;
}

// fn(displayId): the open grid lost the focus.
function onBlur(fn) {
  blurListener = fn;
}

function destroyAll() {
  shownId = null;
  for (const { win } of windows.values()) {
    if (!win.isDestroyed()) win.destroy();
  }
  windows.clear();
}

module.exports = { show, hide, isFrom, onBlur, destroyAll };
