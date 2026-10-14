const test = require('node:test');
const assert = require('node:assert');
require('./helpers'); // stubs out electron, which lib/layouts.js requires
const cells = require('../renderer/cells');
const zones = require('../lib/zones');
const { BUILT_IN_LAYOUTS } = require('../lib/layouts');

const quarters = BUILT_IN_LAYOUTS.find((l) => l.id === 'quarters');
const thirds = BUILT_IN_LAYOUTS.find((l) => l.id === 'thirds');

// What the editor promises: a valid layout whose zones cover every cell
// exactly once.
function assertTiles(layout) {
  assert.strictEqual(zones.validateLayout({ id: 'x', ...layout, name: layout.name || 'x' }), null);
  const area = layout.zones.reduce((sum, z) => sum + z.w * z.h, 0);
  assert.strictEqual(area, layout.cols * layout.rows, 'no holes');
}

const shape = (layout) => layout.zones.map(({ id, x, y, w, h }) => [id, x, y, w, h]);

test('cellSpan: the block between two corners, in either order', () => {
  assert.deepStrictEqual(cells.cellSpan({ x: 1, y: 2 }, { x: 3, y: 0 }), { x: 1, y: 0, w: 3, h: 3 });
  assert.deepStrictEqual(cells.cellSpan({ x: 4, y: 1 }, { x: 4, y: 1 }), { x: 4, y: 1, w: 1, h: 1 });
});

test('moveBlock and resizeBlock never leave the grid', () => {
  const b = { x: 4, y: 2, w: 2, h: 2 };
  assert.deepStrictEqual(cells.moveBlock(b, 1, 1, 6, 4), b, 'already in the corner');
  assert.deepStrictEqual(cells.moveBlock(b, -1, 0, 6, 4), { x: 3, y: 2, w: 2, h: 2 });
  assert.deepStrictEqual(cells.resizeBlock(b, 1, 1, 6, 4), b, 'no room to grow');
  assert.deepStrictEqual(cells.resizeBlock(b, -1, -1, 6, 4), { x: 4, y: 2, w: 1, h: 1 });
  assert.deepStrictEqual(cells.resizeBlock({ x: 4, y: 2, w: 1, h: 1 }, -1, -1, 6, 4), { x: 4, y: 2, w: 1, h: 1 }, 'one cell at least');
  assert.deepStrictEqual(cells.resizeBlock({ x: 0, y: 0, w: 1, h: 1 }, 1, 0, 6, 4), { x: 0, y: 0, w: 2, h: 1 });
});

test('blockFromRect: snaps a window to the nearest grid lines', () => {
  const area = { width: 1200, height: 800 }; // 6 x 4 cells of 200
  assert.deepStrictEqual(cells.blockFromRect({ x: 0, y: 0, width: 1200, height: 800 }, area, 6, 4), { x: 0, y: 0, w: 6, h: 4 }, 'maximized');
  assert.deepStrictEqual(cells.blockFromRect({ x: 190, y: 220, width: 430, height: 370 }, area, 6, 4), { x: 1, y: 1, w: 2, h: 2 });
  assert.deepStrictEqual(cells.blockFromRect({ x: -7, y: -7, width: 614, height: 814 }, area, 6, 4), { x: 0, y: 0, w: 3, h: 4 }, 'invisible borders hang off the edge');
  assert.deepStrictEqual(cells.blockFromRect({ x: 500, y: 300, width: 40, height: 40 }, area, 6, 4), null, 'smaller than half a cell');
  assert.deepStrictEqual(cells.blockFromRect({ x: 520, y: 350, width: 120, height: 120 }, area, 6, 4), { x: 3, y: 2, w: 1, h: 1 }, 'a small window still gets a cell');
});

test('blockFromRect: null for a window that is off the area', () => {
  const area = { width: 1200, height: 800 };
  assert.strictEqual(cells.blockFromRect({ x: -32000, y: -32000, width: 160, height: 28 }, area, 6, 4), null, 'minimized');
  assert.strictEqual(cells.blockFromRect({ x: -1920, y: 0, width: 1000, height: 700 }, area, 6, 4), null, 'another display');
});

test('defaultBlock sits in the middle of the grid', () => {
  assert.deepStrictEqual(cells.defaultBlock(6, 4), { x: 1, y: 1, w: 3, h: 2 });
  assert.deepStrictEqual(cells.defaultBlock(4, 6), { x: 1, y: 1, w: 2, h: 3 });
  assert.deepStrictEqual(cells.defaultBlock(1, 1), { x: 0, y: 0, w: 1, h: 1 });
});

test('blankLayout: one zone per cell', () => {
  const layout = cells.blankLayout('New', 4, 2);
  assertTiles(layout);
  assert.strictEqual(layout.zones.length, 8);
});

test('mergeZones widens the selection so it never cuts a zone in two', () => {
  // quarters, then the top two merged: a wide top zone and two bottom ones
  const top = cells.mergeZones(quarters, { x: 0, y: 0, w: 2, h: 1 });
  assertTiles(top);
  assert.deepStrictEqual(shape(top), [['top-left', 0, 0, 2, 1], ['bottom-left', 0, 1, 1, 1], ['bottom-right', 1, 1, 1, 1]]);

  // Selecting just the left column cuts through the wide top zone, so it
  // takes the whole layout.
  assert.deepStrictEqual(cells.expandToZones(top.zones, { x: 0, y: 0, w: 1, h: 2 }), { x: 0, y: 0, w: 2, h: 2 });
  const all = cells.mergeZones(top, { x: 0, y: 0, w: 1, h: 2 });
  assertTiles(all);
  assert.deepStrictEqual(shape(all), [['top-left', 0, 0, 2, 2]]);
});

test('mergeZones keeps the top-left zone and leaves a single zone alone', () => {
  const merged = cells.mergeZones(thirds, { x: 1, y: 0, w: 2, h: 1 });
  assert.deepStrictEqual(shape(merged), [['left', 0, 0, 1, 1], ['center', 1, 0, 2, 1]]);
  assert.strictEqual(merged.zones[1].name, 'Center');
  assert.strictEqual(cells.mergeZones(thirds, { x: 1, y: 0, w: 1, h: 1 }), thirds);
});

test('splitZones breaks zones back into cells, with fresh ids for the new ones', () => {
  const main = BUILT_IN_LAYOUTS.find((l) => l.id === 'main-stack');
  const split = cells.splitZones(main, { x: 1, y: 1, w: 1, h: 1 });
  assertTiles(split);
  assert.strictEqual(split.zones.length, 8, 'main became 6 cells; the stack zones are outside the selection');
  assert.deepStrictEqual(split.zones.find((z) => z.id === 'main'), { id: 'main', name: 'Main', x: 0, y: 0, w: 1, h: 1 });
  const fresh = split.zones.filter((z) => /^z\d+$/.test(z.id)).map((z) => z.id);
  assert.deepStrictEqual(fresh, ['z1', 'z2', 'z3', 'z4', 'z5']);
  assert.ok(fresh.every((id) => split.zones.find((z) => z.id === id).name === ''));
});

test('resizeGrid scales zones when the new count is a multiple, and cuts them otherwise', () => {
  const focus = BUILT_IN_LAYOUTS.find((l) => l.id === 'focus'); // 3x1: main 2 wide, side 1 wide
  const doubled = cells.resizeGrid(focus, 6, 2);
  assertTiles(doubled);
  assert.deepStrictEqual(shape(doubled), [['main', 0, 0, 4, 2], ['side', 4, 0, 2, 2]]);

  assert.deepStrictEqual(shape(cells.resizeGrid(doubled, 3, 1)), shape(focus), 'and back again');

  const cut = cells.resizeGrid(focus, 2, 1);
  assertTiles(cut);
  assert.deepStrictEqual(shape(cut), [['main', 0, 0, 2, 1]], 'the side zone falls off the edge');

  const grown = cells.resizeGrid(focus, 4, 1);
  assertTiles(grown);
  assert.deepStrictEqual(shape(grown), [['main', 0, 0, 2, 1], ['side', 2, 0, 1, 1], ['z1', 3, 0, 1, 1]], 'a new cell fills the gap');
});

test('resizeGrid only scales down when every zone edge survives', () => {
  const wide = { name: 'Wide', cols: 4, rows: 1, zones: [{ id: 'a', name: '', x: 0, y: 0, w: 1, h: 1 }, { id: 'b', name: '', x: 1, y: 0, w: 3, h: 1 }] };
  const halved = cells.resizeGrid(wide, 2, 1);
  assertTiles(halved);
  assert.deepStrictEqual(shape(halved), [['a', 0, 0, 1, 1], ['b', 1, 0, 1, 1]], 'cut, not scaled: 1 and 3 are not on the new lines');
});

test('nameZones numbers the unnamed zones in reading order; unnameZones undoes it', () => {
  const layout = {
    name: 'L', cols: 2, rows: 2,
    zones: [
      { id: 'b', name: '', x: 1, y: 0, w: 1, h: 2 },
      { id: 'a', name: ' Main ', x: 0, y: 0, w: 1, h: 1 },
      { id: 'c', name: 'Zone 7', x: 0, y: 1, w: 1, h: 1 },
    ],
  };
  const named = cells.nameZones(layout);
  assert.deepStrictEqual(named.zones.map((z) => [z.id, z.name]), [['a', 'Main'], ['b', 'Zone 1'], ['c', 'Zone 2']]);
  assert.deepStrictEqual(cells.unnameZones(named).zones.map((z) => z.name), ['Main', '', '']);
});
