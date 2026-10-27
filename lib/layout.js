// Grid-cell footprint math for the freeform, snap-to-grid canvas used at
// every level (Home, and inside each island). Loose apps and minimized
// islands take one cell; expanded islands take a user-resizable block
// (defaulting to 3x2). Positions/sizes are integer cell coordinates, not
// pixels, and every item's position is scoped to its own parent - siblings
// inside the same island form their own independent coordinate space.

const EXPANDED_ISLAND_W = 3;
const EXPANDED_ISLAND_H = 2;
const MIN_ISLAND_W = 2;
const MIN_ISLAND_H = 2;
const MAX_ISLAND_W = 8;
const MAX_ISLAND_H = 6;

function footprintOf(entry) {
  if (entry.kind === 'island' && entry.collapsed === false) {
    return { w: entry.w || EXPANDED_ISLAND_W, h: entry.h || EXPANDED_ISLAND_H };
  }
  return { w: 1, h: 1 };
}

function rectsOverlap(a, b) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

function isFree(occupied, x, y, w, h) {
  if (x < 0 || y < 0) return false;
  const rect = { x, y, w, h };
  return !occupied.some((o) => rectsOverlap(rect, o));
}

// First free cell scanning top-left to bottom-right, reading order. Used for
// brand-new items and one-time migration so placement is deterministic.
function findFirstFreeCell(occupied, w, h, maxCols = 8) {
  for (let y = 0; y < 10000; y++) {
    for (let x = 0; x < maxCols; x++) {
      if (isFree(occupied, x, y, w, h)) return { x, y };
    }
  }
  return { x: 0, y: 0 };
}

// Nearest free cell to a drop target, spiraling outward ring by ring. Used
// when the user drags something to a spot that's already occupied.
function findNearestFreeCell(occupied, targetX, targetY, w, h) {
  const tx = Math.max(0, targetX);
  const ty = Math.max(0, targetY);
  if (isFree(occupied, tx, ty, w, h)) return { x: tx, y: ty };

  for (let radius = 1; radius <= 200; radius++) {
    for (let dy = -radius; dy <= radius; dy++) {
      for (let dx = -radius; dx <= radius; dx++) {
        if (Math.max(Math.abs(dx), Math.abs(dy)) !== radius) continue; // ring edge only
        const x = Math.max(0, tx + dx);
        const y = Math.max(0, ty + dy);
        if (isFree(occupied, x, y, w, h)) return { x, y };
      }
    }
  }
  return findFirstFreeCell(occupied, w, h);
}

// Cell footprints occupied among the siblings sharing `parentId` (null =
// Home, or an island's id), leaving out `excludeId` (usually the item being
// placed). Built fresh on every call: a cached list is how a batch ends up
// stacking two items on one cell.
function siblingFootprints(apps, parentId, excludeId) {
  const key = parentId || null;
  return apps
    .filter((a) => (a.parentId || null) === key && a.id !== excludeId && typeof a.gridX === 'number' && typeof a.gridY === 'number')
    .map((a) => ({ ...footprintOf(a), x: a.gridX, y: a.gridY }));
}

// Places `entry` (already carrying its parentId) on a free cell among its
// siblings in `apps`: the nearest one to (x, y) when a target is given, the
// first one in reading order otherwise. Writes gridX/gridY onto the entry,
// so the next placement against the same `apps` sees it.
function placeFree(apps, entry, x, y) {
  const fp = footprintOf(entry);
  const occupied = siblingFootprints(apps, entry.parentId, entry.id);
  const pos =
    Number.isFinite(x) && Number.isFinite(y)
      ? findNearestFreeCell(occupied, Math.round(x), Math.round(y), fp.w, fp.h)
      : findFirstFreeCell(occupied, fp.w, fp.h);
  entry.gridX = pos.x;
  entry.gridY = pos.y;
  return pos;
}

module.exports = {
  footprintOf,
  siblingFootprints,
  placeFree,
  rectsOverlap,
  findFirstFreeCell,
  findNearestFreeCell,
  EXPANDED_ISLAND_W,
  EXPANDED_ISLAND_H,
  MIN_ISLAND_W,
  MIN_ISLAND_H,
  MAX_ISLAND_W,
  MAX_ISLAND_H,
};
