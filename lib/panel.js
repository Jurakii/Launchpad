// The organizer panel: a thin bar along the top of every display. With
// "always" it's docked as a Windows AppBar, so the strip it covers comes out
// of the work area and maximized windows stop below it. With "autoHide" it
// reserves nothing and comes down when the cursor rests against the top
// edge. Either way it hides on a display a fullscreen app has taken over.
// What it shows (layouts, the focused window, whether the helper is up)
// comes from lib/organizer.js through setContent(); this file only manages
// the windows.
const path = require('path');
const { BrowserWindow, screen } = require('electron');
const windowmgr = require('./windowmgr');
const autohide = require('./autohide');

const PANEL_HEIGHT = 32; // DIP
// The message Explorer sends this AppBar's notifications with (WM_APP + 0x42).
const APPBAR_MSG = 0x8042;
const ABN_POSCHANGED = 1;
const ABN_FULLSCREENAPP = 2;
// The taskbar or another AppBar moved: they come in bursts.
const REDOCK_DELAY_MS = 100;
const AUTOHIDE_POLL_MS = 50;
// While a panel is hidden for a fullscreen app, or Explorer says one is
// running on its display, how often to look again. A fullscreen window
// closing on a display the user isn't working on doesn't change the
// foreground, and Explorer only notifies when its own idea of "fullscreen"
// changes, which can already be set by something else (a full-screen
// click-through overlay, say) by the time a real fullscreen app opens.
const FULLSCREEN_RECHECK_MS = 2000;
// Apps usually go fullscreen just after they come to the front, so a focus
// change is checked once more after this long.
const FOCUS_RECHECK_MS = 600;

const PANEL_HTML = path.join(__dirname, '..', 'renderer', 'panel.html');
const PANEL_PRELOAD = path.join(__dirname, '..', 'panel-preload.js');

let mode = 'off'; // 'always' | 'autoHide' | 'off'
let content = null;
let dragging = false;
let taskbarCreatedMsg = null;
let watchingDisplays = false;
let autoHideTimer = null;
let recheckTimer = null;
let focusRecheckTimer = null;
let fullscreenCheck = null; // the check in flight
let fullscreenAgain = false; // asked for again while one was in flight
// display id -> { display, win, hwnd, loaded, docked, fullscreen, explorerFullscreen,
//                 auto, checking, redockTimer }
const panels = new Map();

const sameRect = (a, b) => a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
const handleOf = (win) => Number(win.getNativeWindowHandle().readBigUInt64LE(0));
const autoHideRect = (d) => ({ x: d.bounds.x, y: d.bounds.y, width: d.bounds.width, height: PANEL_HEIGHT });

windowmgr.onEvent((ev) => {
  if (ev.ev !== 'ready') return;
  taskbarCreatedMsg = ev.taskbarCreated;
  // A new helper doesn't know what the last one docked, and anything asked
  // of a helper that died was lost, so everything is docked again.
  for (const panel of panels.values()) {
    hookTaskbarCreated(panel);
    place(panel);
  }
  if (panels.size) checkFullscreen();
});

function setMode(next) {
  if (next === mode) return;
  mode = next;
  if (mode === 'off') {
    stopAutoHide();
    clearTimeout(recheckTimer);
    clearTimeout(focusRecheckTimer);
    for (const panel of [...panels.values()]) removePanel(panel);
    return;
  }
  watchDisplays();
  syncPanels();
  for (const panel of panels.values()) {
    panel.auto = autohide.initial();
    place(panel);
  }
  if (mode === 'autoHide') startAutoHide();
  else stopAutoHide();
  checkFullscreen();
}

// { layouts, focused, helperUp }, the same for every panel.
function setContent(next) {
  content = next;
  for (const panel of panels.values()) sendState(panel);
}

// The drag strip takes the top of the display while a window is dragged; an
// auto-hiding panel keeps out of its way.
function setDragging(on) {
  dragging = !!on;
  if (!dragging || mode !== 'autoHide') return;
  for (const panel of panels.values()) {
    panel.auto = autohide.initial();
    updateVisibility(panel);
  }
}

// The (current) display of the panel a message came from, or null if it
// didn't come from a panel.
function displayForSender(sender) {
  for (const panel of panels.values()) {
    if (!panel.win.isDestroyed() && panel.win.webContents === sender) {
      return screen.getAllDisplays().find((d) => d.id === panel.display.id) || null;
    }
  }
  return null;
}

function watchDisplays() {
  if (watchingDisplays) return;
  watchingDisplays = true;
  for (const event of ['display-added', 'display-removed', 'display-metrics-changed']) {
    screen.on(event, () => {
      if (mode !== 'off') syncPanels();
    });
  }
}

// One panel per display. Docking changes the work area, which fires
// display-metrics-changed too, so a panel is only re-placed when its
// display's bounds or scaling changed.
function syncPanels() {
  const displays = screen.getAllDisplays();
  for (const panel of [...panels.values()]) {
    const d = displays.find((x) => x.id === panel.display.id);
    if (!d) {
      removePanel(panel);
      continue;
    }
    const moved = !sameRect(d.bounds, panel.display.bounds) || d.scaleFactor !== panel.display.scaleFactor;
    const reshaped = d.workArea.width !== panel.display.workArea.width || d.workArea.height !== panel.display.workArea.height;
    panel.display = d;
    if (moved) place(panel);
    if (moved || reshaped) sendState(panel); // thumbnails follow the work area's shape
  }
  for (const d of displays) {
    if (!panels.has(d.id)) createPanel(d);
  }
}

function createPanel(display) {
  const win = new BrowserWindow({
    ...autoHideRect(display),
    show: false,
    frame: false,
    transparent: true, // the translucent and transparent looks
    backgroundColor: '#00000000',
    hasShadow: false,
    focusable: false, // clicking it mustn't take the foreground from the window it acts on
    skipTaskbar: true,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    webPreferences: {
      preload: PANEL_PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  // The constructor's alwaysOnTop option doesn't take for a window like this
  // one (checked: no WS_EX_TOPMOST); setting it afterwards does.
  win.setAlwaysOnTop(true, 'screen-saver');
  const panel = {
    display,
    win,
    hwnd: handleOf(win),
    loaded: false,
    docked: false,
    fullscreen: false, // a fullscreen window covers its display: our own check
    explorerFullscreen: false, // Explorer's last ABN_FULLSCREENAPP for it
    auto: autohide.initial(),
    checking: false,
    redockTimer: null,
  };
  win.hookWindowMessage(APPBAR_MSG, (wParam, lParam) => {
    const code = Number(wParam.readBigUInt64LE(0));
    if (code === ABN_POSCHANGED) {
      clearTimeout(panel.redockTimer);
      panel.redockTimer = setTimeout(() => place(panel), REDOCK_DELAY_MS);
    } else if (code === ABN_FULLSCREENAPP) {
      // Only a hint: it can arrive for something that isn't really a
      // fullscreen app, and doesn't say which window. checkFullscreen decides.
      panel.explorerFullscreen = lParam.readBigInt64LE(0) !== 0n;
      checkFullscreen();
    }
  });
  if (taskbarCreatedMsg) hookTaskbarCreated(panel);
  win.webContents.once('did-finish-load', () => {
    panel.loaded = true;
    sendState(panel);
    updateVisibility(panel);
  });
  win.loadFile(PANEL_HTML);
  panels.set(display.id, panel);
  place(panel);
  return panel;
}

// Explorer forgets every AppBar when it restarts.
function hookTaskbarCreated(panel) {
  if (!panel.win.isDestroyed()) panel.win.hookWindowMessage(taskbarCreatedMsg, () => place(panel, true));
}

function removePanel(panel) {
  panels.delete(panel.display.id);
  clearTimeout(panel.redockTimer);
  if (panel.docked) windowmgr.appbarRemove(panel.hwnd).catch(() => {});
  panel.docked = false;
  if (!panel.win.isDestroyed()) panel.win.destroy();
}

const isCurrent = (panel) => panels.get(panel.display.id) === panel && !panel.win.isDestroyed();

// Windows won't make a transparent Electron window shorter than its minimum
// window height (39 px at 100%), so the panel's window ends up taller than
// the strip it reserves. Cutting it to that strip with a window region
// leaves the rest neither drawn nor clickable, so it can't cover the top of
// the maximized windows below.
function setPanelBounds(panel, rect) {
  panel.win.setBounds(rect);
  panel.win.setShape([{ x: 0, y: 0, width: rect.width, height: rect.height }]);
}

// Docks the panel along the top of its display ("always"), or undocks it and
// parks it at the top edge, hidden until revealed ("autoHide").
async function place(panel, renew = false) {
  if (!isCurrent(panel)) return;
  const d = panel.display;
  if (mode !== 'always') {
    if (panel.docked) windowmgr.appbarRemove(panel.hwnd).catch(() => {});
    panel.docked = false;
    setPanelBounds(panel, autoHideRect(d));
    updateVisibility(panel);
    return;
  }
  const phys = screen.dipToScreenRect(null, d.bounds);
  const want = { x: phys.x, y: phys.y, width: phys.width, height: Math.round(PANEL_HEIGHT * d.scaleFactor) };
  let reply;
  try {
    reply = await windowmgr.appbarSet(panel.hwnd, APPBAR_MSG, want, renew);
  } catch {
    return; // the helper died; its replacement's 'ready' docks everything again
  }
  if (!isCurrent(panel) || mode !== 'always') {
    // Turned off or switched to auto-hide while this was on its way.
    if (reply.ok && !panel.docked) windowmgr.appbarRemove(panel.hwnd).catch(() => {});
    return;
  }
  if (!reply.ok) return;
  panel.docked = true;
  const [l, t, r, b] = reply.rect;
  setPanelBounds(panel, screen.screenToDipRect(null, { x: l, y: t, width: r - l, height: b - t }));
  updateVisibility(panel);
}

function updateVisibility(panel) {
  if (panel.win.isDestroyed()) return;
  let show = panel.loaded && !panel.fullscreen;
  if (mode === 'always') show = show && panel.docked; // never over windows that haven't moved out of the way
  else show = show && mode === 'autoHide' && panel.auto.revealed;
  if (show && !panel.win.isVisible()) panel.win.showInactive();
  else if (!show && panel.win.isVisible()) panel.win.hide();
}

function sendState(panel) {
  if (!panel.loaded || !content || panel.win.isDestroyed()) return;
  const wa = panel.display.workArea;
  panel.win.webContents.send('panel:state', { ...content, aspect: wa.width / wa.height, height: PANEL_HEIGHT });
}

// Asks the helper which displays a fullscreen window has taken over and
// hides those displays' panels. Called on focus changes and AppBar
// notifications; calls that arrive while one is running run it once more
// afterwards, since what they're about may have happened too late for it.
function checkFullscreen() {
  // Any request starts the helper, which mustn't happen once the panels are gone.
  if (!panels.size) return Promise.resolve();
  if (fullscreenCheck) {
    fullscreenAgain = true;
    return fullscreenCheck;
  }
  fullscreenCheck = windowmgr
    .fullscreenMonitors()
    .then((reply) => {
      const covered = reply.ok ? reply.monitors : [];
      for (const panel of panels.values()) {
        const p = screen.dipToScreenRect(null, panel.display.bounds);
        panel.fullscreen = covered.some(([l, t, r, b]) => l === p.x && t === p.y && r === p.x + p.width && b === p.y + p.height);
        updateVisibility(panel);
      }
    })
    .catch(() => {})
    .then(() => {
      fullscreenCheck = null;
      clearTimeout(recheckTimer);
      recheckTimer = null;
      if (fullscreenAgain) {
        fullscreenAgain = false;
        return checkFullscreen();
      }
      if ([...panels.values()].some((panel) => panel.fullscreen || panel.explorerFullscreen)) {
        recheckTimer = setTimeout(checkFullscreen, FULLSCREEN_RECHECK_MS);
      }
    });
  return fullscreenCheck;
}

// Another window came to the front: a fullscreen app may have come or gone.
function focusChanged() {
  if (!panels.size) return;
  checkFullscreen();
  clearTimeout(focusRecheckTimer);
  focusRecheckTimer = setTimeout(checkFullscreen, FOCUS_RECHECK_MS);
}

// ---------- auto-hide ----------

function startAutoHide() {
  if (!autoHideTimer) autoHideTimer = setInterval(pollAutoHide, AUTOHIDE_POLL_MS);
}

function stopAutoHide() {
  clearInterval(autoHideTimer);
  autoHideTimer = null;
}

function pollAutoHide() {
  const cursor = screen.getCursorScreenPoint();
  const now = Date.now();
  for (const panel of panels.values()) {
    const next = autohide.step(panel.auto, {
      cursor,
      bounds: panel.display.bounds,
      panel: autoHideRect(panel.display),
      now,
      canReveal: !dragging && !panel.fullscreen && !panel.checking,
    });
    if (next.revealed && !panel.auto.revealed) {
      // No AppBar notification reaches an auto-hiding panel, so a
      // fullscreen app could have taken over unnoticed: look first.
      panel.auto = autohide.initial();
      revealUnlessFullscreen(panel);
      continue;
    }
    panel.auto = next;
    updateVisibility(panel);
  }
}

function revealUnlessFullscreen(panel) {
  panel.checking = true;
  checkFullscreen().then(() => {
    panel.checking = false;
    if (!isCurrent(panel) || mode !== 'autoHide' || panel.fullscreen || dragging) return;
    panel.auto = autohide.initial(true);
    updateVisibility(panel);
  });
}

module.exports = { setMode, setContent, setDragging, displayForSender, focusChanged };
