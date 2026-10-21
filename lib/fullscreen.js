// Whether a fullscreen app has taken over a display, decided from the
// window helper's 'fullscreen' reply (lib/windowmgr.ps1). Pure, so it's
// testable without Electron. Rects are physical pixels.

// The helper names, for each monitor, the window that decides it: the first
// one down the z-order that covers it or isn't always-on-top (or null). It
// takes the monitor over only if it covers all of it, isn't maximized
// (nothing reserves space, so a maximized window filling the monitor is just
// a window), isn't one of Launchpad's own, and is in front: it belongs to the
// foreground app, or Explorer says a fullscreen app is running there
// (ABN_FULLSCREENAPP, which only docked panels hear). A launcher, overlay or
// anti-cheat window a game leaves behind can cover a monitor without either.
function takenOver(win, { explorerHint = false } = {}) {
  if (!win || !win.covers || win.zoomed || win.own) return false;
  return !!win.foreground || explorerHint;
}

const sameRect = ([l, t, r, b], p) => l === p.x && t === p.y && r === p.x + p.width && b === p.y + p.height;

// The new fullscreen state of a display whose bounds (physical) are rect,
// given the helper's reply and its previous state. A failed reply keeps the
// previous state: it says nothing about the display, and assuming "not
// fullscreen" would put the panel over a running game while the helper
// restarts.
function decide(reply, rect, previous, { explorerHint = false } = {}) {
  if (!reply || !reply.ok || !Array.isArray(reply.monitors)) return previous;
  const entry = reply.monitors.find((m) => Array.isArray(m.monitor) && sameRect(m.monitor, rect));
  return entry ? takenOver(entry.window, { explorerHint }) : false;
}

module.exports = { takenOver, decide };
