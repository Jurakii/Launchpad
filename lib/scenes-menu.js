// The scenes menu: a small drop-down under a panel's scenes button with the
// saved scenes (click one to apply it), "Save current arrangement" and
// "Manage scenes…". Like the grid placer it takes the focus, so it works
// from the keyboard and closes when the user clicks anywhere else (blur);
// lib/organizer.js remembers the window the user was in and gives it the
// focus back. There's one window, made on first use and kept hidden, moved
// under whichever panel's button opened it. What it shows and does is up to
// the organizer and main.js; this file only manages the window.
const path = require('path');
const { BrowserWindow } = require('electron');

const MENU_HTML = path.join(__dirname, '..', 'renderer', 'scenes-menu.html');
const MENU_PRELOAD = path.join(__dirname, '..', 'scenes-menu-preload.js');
const WIDTH = 280; // DIP

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
      preload: MENU_PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.setAlwaysOnTop(true, 'screen-saver'); // see createPanel in lib/panel.js
  win.webContents.once('did-finish-load', () => {
    ready = true;
    if (open) win.webContents.send('scenesMenu:state', open.state);
  });
  win.on('blur', () => {
    if (open) blurListener();
  });
  win.loadFile(MENU_HTML);
  return win;
}

// The menu's top right corner goes under the button's right edge (anchor:
// the button's rect in screen DIP), kept on the display.
function bounds(height) {
  const { display, anchor } = open;
  const d = display.bounds;
  const x = Math.max(d.x, Math.min(Math.round(anchor.x + anchor.width) - WIDTH, d.x + d.width - WIDTH));
  const y = Math.round(anchor.y + anchor.height);
  return { x, y, width: WIDTH, height: Math.max(40, Math.min(Math.round(height), d.y + d.height - y)) };
}

// Opens it under a panel button with what to show ({ scenes }, see
// renderer/scenes-menu.js). It appears once the page has said how tall it is.
function show(display, anchor, state) {
  ensureWindow();
  open = { display, anchor, state, sized: false };
  if (ready) win.webContents.send('scenesMenu:state', state);
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

const isOpen = () => !!open;

// Whether a message came from the menu.
function isFrom(sender) {
  return !!win && !win.isDestroyed() && win.webContents === sender;
}

// fn(): the open menu lost the focus.
function onBlur(fn) {
  blurListener = fn;
}

function destroy() {
  open = null;
  if (win && !win.isDestroyed()) win.destroy();
  win = null;
}

module.exports = { show, resize, hide, isOpen, isFrom, onBlur, destroy };
