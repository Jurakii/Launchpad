// The window picker: a small drop-down under a panel's zone or grid button
// listing the open windows, for when there's no window to move (Launchpad
// itself, or the desktop, is in front). Picking one moves it where the user
// clicked. Like the scenes menu (lib/scenes-menu.js), which this copies, it
// takes the focus, so it works from the keyboard and closes when the user
// clicks anywhere else (blur). There's one window, made on first use and
// kept hidden, moved under whichever panel opened it. What it lists and
// does is up to lib/organizer.js; this file only manages the window.
const path = require('path');
const { BrowserWindow } = require('electron');

const PICKER_HTML = path.join(__dirname, '..', 'renderer', 'window-picker.html');
const PICKER_PRELOAD = path.join(__dirname, '..', 'window-picker-preload.js');
const WIDTH = 320; // DIP

let win = null;
let ready = false;
let open = null; // { display, anchor, state, sized } while it's showing (or about to)
let blurListener = () => {};

function ensureWindow() {
  if (win && !win.isDestroyed()) return win;
  ready = false;
  win = new BrowserWindow({
    width: WIDTH,
    height: 120,
    show: false,
    frame: false,
    transparent: true,
    backgroundColor: '#00000000',
    hasShadow: false,
    type: 'toolbar', // out of Alt+Tab and the helper's fullscreen check, like the grid
    skipTaskbar: true,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    webPreferences: {
      preload: PICKER_PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.setAlwaysOnTop(true, 'screen-saver'); // see createPanel in lib/panel.js
  win.webContents.once('did-finish-load', () => {
    ready = true;
    if (open) win.webContents.send('picker:state', open.state);
  });
  win.on('blur', () => {
    if (open) blurListener();
  });
  win.loadFile(PICKER_HTML);
  return win;
}

// The picker's top left corner goes under the left edge of what was clicked
// (anchor: its rect in screen DIP), kept on the display.
function bounds(height) {
  const { display, anchor } = open;
  const d = display.bounds;
  const x = Math.max(d.x, Math.min(Math.round(anchor.x), d.x + d.width - WIDTH));
  const y = Math.round(anchor.y + anchor.height);
  return { x, y, width: WIDTH, height: Math.max(40, Math.min(Math.round(height), d.y + d.height - y)) };
}

// Opens it under a panel with what to show ({ heading, windows }, see
// renderer/window-picker.js). It appears once the page has said how tall it is.
function show(display, anchor, state) {
  ensureWindow();
  open = { display, anchor, state, sized: false };
  if (ready) win.webContents.send('picker:state', state);
}

// From the page after each render: its height in CSS px.
function resize(sender, height) {
  if (!open || !isFrom(sender) || !Number.isFinite(height)) return;
  win.setBounds(bounds(height));
  if (!open.sized) {
    open.sized = true;
    win.show();
    win.focus();
  }
}

function hide() {
  open = null; // first, so hiding doesn't report a blur
  if (win && !win.isDestroyed()) win.hide();
}

// Whether a message came from the picker.
function isFrom(sender) {
  return !!win && !win.isDestroyed() && win.webContents === sender;
}

// fn(): the open picker lost the focus.
function onBlur(fn) {
  blurListener = fn;
}

function destroy() {
  open = null;
  if (win && !win.isDestroyed()) win.destroy();
  win = null;
}

module.exports = { show, resize, hide, isFrom, onBlur, destroy };
