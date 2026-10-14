// Cell math for the window organizer's pages: the layout editor in the main
// window (renderer.js) and the grid placer (grid.js). A block is a rectangle
// of whole cells { x, y, w, h } on a cols x rows grid, the same model as a
// zone in lib/zones.js. No DOM and no Electron: the pages load this with a
// <script> tag, so the functions below become globals there, and the tests
// require() it.

const clampCells = (n, min, max) => Math.min(max, Math.max(min, n));
const blocksOverlap = (a, b) => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
const blockInside = (inner, outer) =>
  inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.w <= outer.x + outer.w && inner.y + inner.h <= outer.y + outer.h;
const byReadingOrder = (a, b) => a.y - b.y || a.x - b.x;

// ---------- the grid placer ----------

// The block between two corner cells, given in either order.
function cellSpan(a, b) {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x) + 1, h: Math.abs(a.y - b.y) + 1 };
}

// Arrow keys: one cell over, same size, never off the grid.
function moveBlock(block, dx, dy, cols, rows) {
  return { ...block, x: clampCells(block.x + dx, 0, cols - block.w), y: clampCells(block.y + dy, 0, rows - block.h) };
}

// Shift+arrows: right and down grow the block, left and up shrink it. The
// top-left corner stays put, as in gTile.
function resizeBlock(block, dw, dh, cols, rows) {
  return { ...block, w: clampCells(block.w + dw, 1, cols - block.x), h: clampCells(block.h + dh, 1, rows - block.y) };
}

// The block closest to rect, a window's frame relative to the area the grid
// covers ({ width, height }): each edge moves to the nearest grid line, and
// what's off the area is cut off first. Null when less than half a cell of
// the window is on the area (minimized, or on another display).
function blockFromRect(rect, area, cols, rows) {
  const left = Math.max(rect.x, 0);
  const top = Math.max(rect.y, 0);
  const right = Math.min(rect.x + rect.width, area.width);
  const bottom = Math.min(rect.y + rect.height, area.height);
  if (right - left < area.width / cols / 2 || bottom - top < area.height / rows / 2) return null;
  const x = clampCells(Math.round((left / area.width) * cols), 0, cols - 1);
  const y = clampCells(Math.round((top / area.height) * rows), 0, rows - 1);
  const x2 = clampCells(Math.round((right / area.width) * cols), x + 1, cols);
  const y2 = clampCells(Math.round((bottom / area.height) * rows), y + 1, rows);
  return { x, y, w: x2 - x, h: y2 - y };
}

// Where the selection starts when the window's own spot can't be used.
function defaultBlock(cols, rows) {
  const w = Math.max(1, Math.round(cols / 2));
  const h = Math.max(1, Math.round(rows / 2));
  return { x: Math.floor((cols - w) / 2), y: Math.floor((rows - h) / 2), w, h };
}

// ---------- the layout editor ----------
// Every cell of a layout in the editor belongs to exactly one zone, so
// merging and splitting can never leave a hole or an overlap. Zones keep
// their ids through edits (the top-left part keeps it), since anything
// saved against a layout, like a scene, refers to its zones by id.

// The smallest block containing `block` that doesn't cut through a zone.
function expandToZones(zones, block) {
  let b = { ...block };
  for (let grew = true; grew; ) {
    grew = false;
    for (const z of zones) {
      if (!blocksOverlap(z, b) || blockInside(z, b)) continue;
      const x = Math.min(b.x, z.x);
      const y = Math.min(b.y, z.y);
      b = { x, y, w: Math.max(b.x + b.w, z.x + z.w) - x, h: Math.max(b.y + b.h, z.y + z.h) - y };
      grew = true;
    }
  }
  return b;
}

const zonesIn = (zones, block) => zones.filter((z) => blocksOverlap(z, block));

function nextZoneId(zones) {
  let max = 0;
  for (const z of zones) {
    const m = /^z(\d+)$/.exec(z.id);
    if (m) max = Math.max(max, Number(m[1]));
  }
  return `z${max + 1}`;
}

// A layout of cols x rows single-cell zones, ready to be merged.
function blankLayout(name, cols, rows) {
  const zones = [];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) zones.push({ id: `z${zones.length + 1}`, name: '', x, y, w: 1, h: 1 });
  }
  return { name, cols, rows, zones };
}

// One zone in place of every zone the block touches (after widening it so
// it cuts through none). The top-left zone's id and name carry over.
function mergeZones(layout, block) {
  const b = expandToZones(layout.zones, block);
  const covered = zonesIn(layout.zones, b).sort(byReadingOrder);
  if (covered.length < 2) return layout;
  const merged = { ...covered[0], x: b.x, y: b.y, w: b.w, h: b.h };
  return { ...layout, zones: [...layout.zones.filter((z) => !blocksOverlap(z, b)), merged].sort(byReadingOrder) };
}

// Every zone the block touches, back into single cells. Each zone's
// top-left cell keeps its id and name.
function splitZones(layout, block) {
  const out = [];
  const taken = [...layout.zones];
  for (const z of layout.zones) {
    if (!blocksOverlap(z, block) || z.w * z.h === 1) {
      out.push(z);
      continue;
    }
    for (let y = z.y; y < z.y + z.h; y++) {
      for (let x = z.x; x < z.x + z.w; x++) {
        if (x === z.x && y === z.y) {
          out.push({ ...z, w: 1, h: 1 });
          continue;
        }
        const cell = { id: nextZoneId(taken), name: '', x, y, w: 1, h: 1 };
        taken.push(cell);
        out.push(cell);
      }
    }
  }
  return { ...layout, zones: out.sort(byReadingOrder) };
}

// A different number of columns or rows. When the new count is a multiple
// of the old one (or the other way round, with every zone edge on a line
// that survives), zones are scaled and nothing is lost. Otherwise zones are
// cut at the new edge, and cells that end up in no zone become zones.
function resizeGrid(layout, cols, rows) {
  const axis = (zones, from, to, pos, size) => {
    if (to % from === 0) {
      const f = to / from;
      return zones.map((z) => ({ ...z, [pos]: z[pos] * f, [size]: z[size] * f }));
    }
    const f = from / to;
    if (from % to === 0 && zones.every((z) => z[pos] % f === 0 && z[size] % f === 0)) {
      return zones.map((z) => ({ ...z, [pos]: z[pos] / f, [size]: z[size] / f }));
    }
    return zones.filter((z) => z[pos] < to).map((z) => ({ ...z, [size]: Math.min(z[size], to - z[pos]) }));
  };
  const zones = axis(axis(layout.zones, layout.cols, cols, 'x', 'w'), layout.rows, rows, 'y', 'h');
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      if (!zones.some((z) => blockInside({ x, y, w: 1, h: 1 }, z))) {
        zones.push({ id: nextZoneId(zones), name: '', x, y, w: 1, h: 1 });
      }
    }
  }
  return { ...layout, cols, rows, zones: zones.sort(byReadingOrder) };
}

// Zones the user hasn't named are called "Zone 1", "Zone 2"… in reading
// order when the layout is saved. The editor treats those names as blank
// again, so the numbers follow later merges and splits.
const AUTO_ZONE_NAME = /^Zone \d+$/;

function nameZones(layout) {
  let n = 0;
  const zones = [...layout.zones]
    .sort(byReadingOrder)
    .map((z) => (z.name && z.name.trim() && !AUTO_ZONE_NAME.test(z.name.trim()) ? { ...z, name: z.name.trim() } : { ...z, name: `Zone ${++n}` }));
  return { ...layout, zones };
}

function unnameZones(layout) {
  return { ...layout, zones: layout.zones.map((z) => (AUTO_ZONE_NAME.test(z.name || '') ? { ...z, name: '' } : z)) };
}

if (typeof module !== 'undefined') {
  module.exports = {
    cellSpan,
    moveBlock,
    resizeBlock,
    blockFromRect,
    defaultBlock,
    expandToZones,
    zonesIn,
    blankLayout,
    mergeZones,
    splitZones,
    resizeGrid,
    nameZones,
    unnameZones,
  };
}
