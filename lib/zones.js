// Zone math for the window organizer. A layout divides a display's work area
// into a cols x rows grid, and each zone is a block of whole cells - the same
// integer-cell idea as lib/layout.js - so a layout means the same thing on
// any resolution. Rects here are in DIP, matching Electron's screen API;
// main converts them to physical pixels just before moving a window.

const { rectsOverlap } = require('./layout');

const MAX_CELLS = 12;

const isCellCount = (n) => Number.isInteger(n) && n >= 1 && n <= MAX_CELLS;

// Returns null for a usable layout, or a short description of the first
// problem found (used to skip bad entries in a hand-edited layouts.json).
function validateLayout(layout) {
  if (!layout || typeof layout !== 'object') return 'not an object';
  if (typeof layout.id !== 'string' || !layout.id) return 'missing id';
  if (typeof layout.name !== 'string' || !layout.name.trim()) return 'missing name';
  if (!isCellCount(layout.cols) || !isCellCount(layout.rows)) {
    return `cols and rows must be whole numbers from 1 to ${MAX_CELLS}`;
  }
  if (!Array.isArray(layout.zones) || layout.zones.length === 0) return 'no zones';

  const ids = new Set();
  for (const z of layout.zones) {
    if (!z || typeof z.id !== 'string' || !z.id) return 'a zone is missing its id';
    if (ids.has(z.id)) return `zone id "${z.id}" is used twice`;
    ids.add(z.id);
    const whole = [z.x, z.y, z.w, z.h].every(Number.isInteger);
    if (!whole || z.x < 0 || z.y < 0 || z.w < 1 || z.h < 1) {
      return `zone "${z.id}" needs whole-number x, y, w and h`;
    }
    if (z.x + z.w > layout.cols || z.y + z.h > layout.rows) {
      return `zone "${z.id}" runs outside the ${layout.cols}x${layout.rows} grid`;
    }
  }
  for (let i = 0; i < layout.zones.length; i++) {
    for (let j = i + 1; j < layout.zones.length; j++) {
      const a = layout.zones[i];
      const b = layout.zones[j];
      if (rectsOverlap(a, b)) return `zones "${a.id}" and "${b.id}" overlap`;
    }
  }
  return null;
}

// The rect a window should fill for `zone` on a display with `workArea`.
// `gap` separates neighbouring zones and also insets the outer edges, so a
// window never touches the screen edge or its neighbour. Grid lines are
// rounded once and shared by the zones on either side, and the gap is split
// ceil/floor across them, so neighbours end up exactly `gap` apart even when
// the work area doesn't divide evenly.
function zoneRect(layout, zone, workArea, gap = 0) {
  const colLine = (i) => workArea.x + Math.round((i * workArea.width) / layout.cols);
  const rowLine = (i) => workArea.y + Math.round((i * workArea.height) / layout.rows);
  const before = Math.ceil(gap / 2);
  const after = Math.floor(gap / 2);

  const left = colLine(zone.x) + (zone.x === 0 ? gap : before);
  const right = colLine(zone.x + zone.w) - (zone.x + zone.w === layout.cols ? gap : after);
  const top = rowLine(zone.y) + (zone.y === 0 ? gap : before);
  const bottom = rowLine(zone.y + zone.h) - (zone.y + zone.h === layout.rows ? gap : after);

  return { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) };
}

module.exports = { validateLayout, zoneRect, MAX_CELLS };
