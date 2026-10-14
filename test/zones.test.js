const test = require('node:test');
const assert = require('node:assert');
require('./helpers'); // stubs out electron, which lib/layouts.js requires
const zones = require('../lib/zones');
const { BUILT_IN_LAYOUTS } = require('../lib/layouts');

const halves = BUILT_IN_LAYOUTS.find((l) => l.id === 'halves');
const right = (r) => r.x + r.width;
const bottom = (r) => r.y + r.height;

// Work areas worth covering: a 1440p monitor under a 32px top bar, a second
// monitor left of the primary (negative x), and sizes that don't divide
// evenly by 2, 3 or 5.
const WORK_AREAS = [
  { x: 0, y: 32, width: 2560, height: 1408 },
  { x: -1920, y: 0, width: 1920, height: 1040 },
  { x: 0, y: 0, width: 1366, height: 728 },
  { x: 7, y: 3, width: 1001, height: 777 },
];

test('every built-in layout is valid', () => {
  for (const layout of BUILT_IN_LAYOUTS) {
    assert.strictEqual(zones.validateLayout(layout), null, layout.id);
  }
});

test('zoneRect: halves with no gap split the work area exactly', () => {
  const wa = { x: 0, y: 32, width: 2560, height: 1408 };
  assert.deepStrictEqual(zones.zoneRect(halves, halves.zones[0], wa, 0), { x: 0, y: 32, width: 1280, height: 1408 });
  assert.deepStrictEqual(zones.zoneRect(halves, halves.zones[1], wa, 0), { x: 1280, y: 32, width: 1280, height: 1408 });
});

test('zoneRect: the gap insets the screen edges and separates neighbours by exactly the gap', () => {
  const wa = { x: 0, y: 32, width: 2560, height: 1408 };
  const l = zones.zoneRect(halves, halves.zones[0], wa, 8);
  const r = zones.zoneRect(halves, halves.zones[1], wa, 8);
  assert.deepStrictEqual(l, { x: 8, y: 40, width: 1268, height: 1392 });
  assert.deepStrictEqual(r, { x: 1284, y: 40, width: 1268, height: 1392 });
  assert.strictEqual(r.x - right(l), 8);

  // An odd gap is split 3 + 2 across the shared line, still 5 apart in total.
  const l5 = zones.zoneRect(halves, halves.zones[0], wa, 5);
  const r5 = zones.zoneRect(halves, halves.zones[1], wa, 5);
  assert.strictEqual(r5.x - right(l5), 5);
});

test('zoneRect: with no gap, every built-in tiles the work area with no overlap or holes', () => {
  for (const layout of BUILT_IN_LAYOUTS) {
    for (const wa of WORK_AREAS) {
      const rects = layout.zones.map((z) => zones.zoneRect(layout, z, wa, 0));
      const area = rects.reduce((sum, r) => sum + r.width * r.height, 0);
      assert.strictEqual(area, wa.width * wa.height, `${layout.id} on ${JSON.stringify(wa)}`);
      for (const r of rects) {
        assert.ok(r.x >= wa.x && r.y >= wa.y && right(r) <= right(wa) && bottom(r) <= bottom(wa), 'inside the work area');
      }
      for (let i = 0; i < rects.length; i++) {
        for (let j = i + 1; j < rects.length; j++) {
          const [a, b] = [rects[i], rects[j]];
          const apart = right(a) <= b.x || right(b) <= a.x || bottom(a) <= b.y || bottom(b) <= a.y;
          assert.ok(apart, `${layout.id}: zones ${i} and ${j} overlap`);
        }
      }
    }
  }
});

test('zoneRect: with a gap, zones keep that distance from the edges and from each other', () => {
  for (const gap of [4, 8, 13]) {
    for (const layout of BUILT_IN_LAYOUTS) {
      for (const wa of WORK_AREAS) {
        const rects = layout.zones.map((z) => zones.zoneRect(layout, z, wa, gap));
        for (const r of rects) {
          assert.ok(r.x - wa.x >= gap && r.y - wa.y >= gap, `${layout.id}: top/left edge`);
          assert.ok(right(wa) - right(r) >= gap && bottom(wa) - bottom(r) >= gap, `${layout.id}: bottom/right edge`);
        }
        for (let i = 0; i < rects.length; i++) {
          for (let j = i + 1; j < rects.length; j++) {
            const [a, b] = [rects[i], rects[j]];
            const dx = Math.max(b.x - right(a), a.x - right(b));
            const dy = Math.max(b.y - bottom(a), a.y - bottom(b));
            assert.ok(Math.max(dx, dy) >= gap, `${layout.id} gap ${gap}: zones ${i} and ${j} are ${Math.max(dx, dy)} apart`);
          }
        }
      }
    }
  }
});

test('validateLayout rejects layouts that would place windows badly', () => {
  const base = () => structuredClone(halves);
  const withZones = (...zs) => ({ ...base(), zones: zs });
  const z = (id, x, y, w, h) => ({ id, x, y, w, h });

  assert.match(zones.validateLayout(null), /not an object/);
  assert.match(zones.validateLayout({ ...base(), id: '' }), /missing id/);
  assert.match(zones.validateLayout({ ...base(), name: '  ' }), /missing name/);
  assert.match(zones.validateLayout({ ...base(), cols: 0 }), /cols and rows/);
  assert.match(zones.validateLayout({ ...base(), rows: 13 }), /cols and rows/);
  assert.match(zones.validateLayout({ ...base(), cols: 1.5 }), /cols and rows/);
  assert.match(zones.validateLayout(withZones()), /no zones/);
  assert.match(zones.validateLayout(withZones(z('a', 0, 0, 1, 1), z('a', 1, 0, 1, 1))), /used twice/);
  assert.match(zones.validateLayout(withZones(z('a', 0, 0, 0.5, 1))), /whole-number/);
  assert.match(zones.validateLayout(withZones(z('a', -1, 0, 1, 1))), /whole-number/);
  assert.match(zones.validateLayout(withZones(z('a', 1, 0, 2, 1))), /outside the 2x1 grid/);
  assert.match(zones.validateLayout(withZones(z('a', 0, 0, 2, 1), z('b', 1, 0, 1, 1))), /"a" and "b" overlap/);
  assert.strictEqual(zones.validateLayout(withZones(z('only', 0, 0, 1, 1))), null, 'a layout may leave cells empty');
});
