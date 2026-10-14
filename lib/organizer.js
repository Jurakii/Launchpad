// Snaps other apps' windows into layout zones. Zones are worked out in DIP
// on the Electron side (lib/zones.js) and converted to physical pixels just
// before the helper moves the window, so mixed-DPI monitors line up.
const { screen } = require('electron');
const layouts = require('./layouts');
const zones = require('./zones');
const windowmgr = require('./windowmgr');

const ERROR_MESSAGES = {
  gone: 'That window has closed.',
  denied: "Windows won't let Launchpad move this window because the app is running as administrator.",
};
const GENERIC_ERROR = "Launchpad couldn't move that window.";

const failure = (reply) => ({ ok: false, error: ERROR_MESSAGES[reply && reply.code] || GENERIC_ERROR });

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

// Moves the window into a zone on the monitor it's already on, then brings
// it to the front so the result is visible over Launchpad. Resolves to
// { ok } or { ok: false, error } with a message ready to show the user.
async function snapWindow(handle, layoutId, zoneId) {
  const { layouts: all, gap } = layouts.load();
  const layout = all.find((l) => l.id === layoutId);
  const zone = layout && layout.zones.find((z) => z.id === zoneId);
  if (!zone) return { ok: false, error: 'That layout no longer exists.' };

  const monitor = await windowmgr.monitorOf(handle);
  if (!monitor.ok) return failure(monitor);
  const display = displayForMonitor(monitor.bounds);
  const target = zones.zoneRect(layout, zone, display.workArea, gap);

  const placed = await windowmgr.place(handle, screen.dipToScreenRect(null, target));
  if (!placed.ok) return failure(placed);
  await windowmgr.focus(handle); // best effort: Windows can refuse to change the foreground window
  return { ok: true };
}

module.exports = { snapWindow };
