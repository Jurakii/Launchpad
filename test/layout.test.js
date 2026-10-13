const test = require('node:test');
const assert = require('node:assert');
const layout = require('../lib/layout');

const cell = (x, y, w = 1, h = 1) => ({ x, y, w, h });

const overlaps = (a, b) => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;

// Small seeded PRNG (mulberry32) so the randomized cases are repeatable.
function rng(seed) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomOccupied(rand, count, span) {
  return Array.from({ length: count }, () =>
    cell(Math.floor(rand() * span), Math.floor(rand() * span), 1 + Math.floor(rand() * 3), 1 + Math.floor(rand() * 2))
  );
}

test('footprintOf: apps and collapsed islands are 1x1, expanded islands use their size', () => {
  assert.deepStrictEqual(layout.footprintOf({ kind: 'app' }), { w: 1, h: 1 });
  assert.deepStrictEqual(layout.footprintOf({ kind: 'island', collapsed: true, w: 5, h: 4 }), { w: 1, h: 1 });
  assert.deepStrictEqual(layout.footprintOf({ kind: 'island' }), { w: 1, h: 1 }, 'collapsed is the default');
  assert.deepStrictEqual(layout.footprintOf({ kind: 'island', collapsed: false, w: 5, h: 4 }), { w: 5, h: 4 });
  assert.deepStrictEqual(layout.footprintOf({ kind: 'island', collapsed: false }), {
    w: layout.EXPANDED_ISLAND_W,
    h: layout.EXPANDED_ISLAND_H,
  });
});

test('findFirstFreeCell fills in reading order and wraps after maxCols', () => {
  assert.deepStrictEqual(layout.findFirstFreeCell([], 1, 1), { x: 0, y: 0 });
  assert.deepStrictEqual(layout.findFirstFreeCell([cell(0, 0), cell(1, 0)], 1, 1), { x: 2, y: 0 });
  assert.deepStrictEqual(layout.findFirstFreeCell([cell(0, 0), cell(2, 0)], 1, 1), { x: 1, y: 0 }, 'fills gaps');

  const fullRow = Array.from({ length: 8 }, (_, x) => cell(x, 0));
  assert.deepStrictEqual(layout.findFirstFreeCell(fullRow, 1, 1), { x: 0, y: 1 });
  assert.deepStrictEqual(layout.findFirstFreeCell([cell(0, 0), cell(1, 0), cell(2, 0)], 1, 1, 3), { x: 0, y: 1 });
});

test('findFirstFreeCell finds room for a whole multi-cell footprint', () => {
  // A 2x2 can't start at (0,0) because (1,1) is taken, nor at (1,0) or (2,0).
  assert.deepStrictEqual(layout.findFirstFreeCell([cell(1, 1)], 2, 2), { x: 2, y: 0 });
  // Something big in the way: the next spot is beside it, not on top of it.
  assert.deepStrictEqual(layout.findFirstFreeCell([cell(0, 0, 3, 2)], 3, 2), { x: 3, y: 0 });
});

test('findFirstFreeCell never overlaps what is already placed', () => {
  const rand = rng(1);
  const placed = [];
  for (let i = 0; i < 200; i++) {
    const w = 1 + Math.floor(rand() * 3);
    const h = 1 + Math.floor(rand() * 2);
    const pos = layout.findFirstFreeCell(placed, w, h);
    const rect = cell(pos.x, pos.y, w, h);
    assert.ok(!placed.some((p) => overlaps(p, rect)), `item ${i} overlaps`);
    placed.push(rect);
  }
});

test('findNearestFreeCell keeps a free drop target and clamps negative targets', () => {
  assert.deepStrictEqual(layout.findNearestFreeCell([cell(0, 0)], 4, 3, 1, 1), { x: 4, y: 3 });
  assert.deepStrictEqual(layout.findNearestFreeCell([], -3, -1, 1, 1), { x: 0, y: 0 });
});

test('findNearestFreeCell moves an occupied drop to an adjacent free cell', () => {
  const pos = layout.findNearestFreeCell([cell(5, 5)], 5, 5, 1, 1);
  assert.strictEqual(Math.max(Math.abs(pos.x - 5), Math.abs(pos.y - 5)), 1);
});

test('findNearestFreeCell returns a non-overlapping cell at the smallest possible distance', () => {
  const rand = rng(7);
  for (let round = 0; round < 300; round++) {
    const occupied = randomOccupied(rand, 25, 10);
    const w = 1 + Math.floor(rand() * 3);
    const h = 1 + Math.floor(rand() * 2);
    const tx = Math.floor(rand() * 12) - 1;
    const ty = Math.floor(rand() * 12) - 1;

    const pos = layout.findNearestFreeCell(occupied, tx, ty, w, h);
    const rect = cell(pos.x, pos.y, w, h);
    assert.ok(pos.x >= 0 && pos.y >= 0, 'never negative');
    assert.ok(!occupied.some((o) => overlaps(o, rect)), `round ${round}: overlaps`);

    // Brute-force the closest free spot (Chebyshev distance from the
    // clamped target) and check nothing nearer was skipped.
    const cx = Math.max(0, tx);
    const cy = Math.max(0, ty);
    const dist = (x, y) => Math.max(Math.abs(x - cx), Math.abs(y - cy));
    let best = Infinity;
    for (let y = 0; y < 20; y++) {
      for (let x = 0; x < 20; x++) {
        const r = cell(x, y, w, h);
        if (!occupied.some((o) => overlaps(o, r))) best = Math.min(best, dist(x, y));
      }
    }
    assert.strictEqual(dist(pos.x, pos.y), best, `round ${round}: a nearer free cell existed`);
  }
});
