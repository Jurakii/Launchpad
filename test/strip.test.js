const test = require('node:test');
const assert = require('node:assert');
require('./helpers'); // stubs out electron, which lib/layouts.js requires
const strip = require('../lib/strip');
const { BUILT_IN_LAYOUTS } = require('../lib/layouts');

const WA = { x: 0, y: 32, width: 2560, height: 1408 }; // primary under a 32px top bar
const SECOND = { x: -1920, y: 0, width: 1920, height: 1080 }; // left of the primary
const right = (r) => r.x + r.width;
const bottom = (r) => r.y + r.height;
const inside = (inner, outer) =>
  inner.x >= outer.x && inner.y >= outer.y && right(inner) <= right(outer) && bottom(inner) <= bottom(outer);
const centre = (r) => ({ x: r.x + r.width / 2, y: r.y + r.height / 2 });

test('computeStrip: one thumbnail per layout, centred along the top of the work area', () => {
  for (const wa of [WA, SECOND]) {
    const s = strip.computeStrip(BUILT_IN_LAYOUTS, wa);
    assert.deepStrictEqual(s.items.map((i) => i.layoutId), BUILT_IN_LAYOUTS.map((l) => l.id));
    assert.strictEqual(s.rect.y, wa.y);
    assert.ok(Math.abs(centre(s.rect).x - centre(wa).x) <= 1, 'centred');
    assert.ok(inside(s.rect, wa), 'inside the work area');
    for (const item of s.items) {
      assert.ok(inside(item.thumb, s.rect), `${item.layoutId} thumbnail inside the strip`);
      for (const z of item.zones) assert.ok(inside(z.rect, item.thumb), `${item.layoutId}/${z.zoneId} inside its thumbnail`);
    }
    for (let i = 1; i < s.items.length; i++) {
      assert.ok(s.items[i].thumb.x >= right(s.items[i - 1].thumb), 'thumbnails left to right without overlapping');
    }
  }
});

test('computeStrip: thumbnails share the display shape and shrink to fit a narrow display', () => {
  const wide = strip.computeStrip(BUILT_IN_LAYOUTS, WA).items[0].thumb;
  assert.strictEqual(wide.width, 96);
  assert.strictEqual(wide.height, Math.round((96 * WA.height) / WA.width));

  const narrow = { x: 0, y: 0, width: 400, height: 800 };
  const s = strip.computeStrip(BUILT_IN_LAYOUTS, narrow);
  assert.ok(s.items[0].thumb.width < 96);
  assert.ok(s.rect.width <= narrow.width);
});

test('zoneAt: finds the zone under the cursor, and nothing between or outside thumbnails', () => {
  const s = strip.computeStrip(BUILT_IN_LAYOUTS, WA);
  const quarters = s.items.find((i) => i.layoutId === 'quarters');
  for (const z of quarters.zones) {
    assert.deepStrictEqual(strip.zoneAt(s, centre(z.rect)), { layoutId: 'quarters', zoneId: z.zoneId });
  }
  // Every point of a thumbnail belongs to some zone, including the edges.
  const halves = s.items.find((i) => i.layoutId === 'halves');
  assert.deepStrictEqual(strip.zoneAt(s, { x: halves.thumb.x, y: halves.thumb.y }), { layoutId: 'halves', zoneId: 'left' });
  assert.deepStrictEqual(strip.zoneAt(s, { x: right(halves.thumb) - 1, y: bottom(halves.thumb) - 1 }), { layoutId: 'halves', zoneId: 'right' });

  const betweenThumbs = { x: right(s.items[0].thumb) + 2, y: centre(s.items[0].thumb).y };
  assert.strictEqual(strip.zoneAt(s, betweenThumbs), null);
  assert.strictEqual(strip.zoneAt(s, { x: centre(WA).x, y: centre(WA).y }), null);
});

test('shouldShow: appears near the top, stays while near the strip, hides once the cursor leaves', () => {
  const s = strip.computeStrip(BUILT_IN_LAYOUTS, WA);
  const midX = centre(s.rect).x;
  const belowTrigger = WA.y + strip.TRIGGER_DIP + 5;

  assert.strictEqual(strip.shouldShow(s, WA, { x: midX, y: WA.y + 10 }, false), true, 'near the top');
  assert.strictEqual(strip.shouldShow(s, WA, { x: 200, y: WA.y + 10 }, false), true, 'anywhere along the top');
  assert.strictEqual(strip.shouldShow(s, WA, { x: midX, y: WA.y - 20 }, false), true, 'above the work area (top taskbar)');
  assert.strictEqual(strip.shouldShow(s, WA, { x: 200, y: belowTrigger }, false), false, 'not before reaching the top');
  assert.strictEqual(strip.shouldShow(s, WA, { x: midX, y: bottom(s.rect) + 5 }, false), false, 'not just below the strip');
  // A flick can skip the band entirely between polls; landing on the strip still shows it.
  const thumb = s.items[2].thumb;
  const lowOnThumb = { x: centre(thumb).x, y: bottom(thumb) - 2 };
  assert.ok(lowOnThumb.y >= WA.y + strip.TRIGGER_DIP, 'the test needs a point below the trigger band');
  assert.strictEqual(strip.shouldShow(s, WA, lowOnThumb, false), true, 'over the strip area');

  // Once showing, it stays within a margin around the strip.
  assert.strictEqual(strip.shouldShow(s, WA, lowOnThumb, true), true);
  assert.strictEqual(strip.shouldShow(s, WA, { x: midX, y: bottom(s.rect) + strip.STAY_MARGIN_DIP - 1 }, true), true, 'within the margin');
  assert.strictEqual(strip.shouldShow(s, WA, { x: midX, y: bottom(s.rect) + strip.STAY_MARGIN_DIP + 1 }, true), false, 'past the margin');
  assert.strictEqual(strip.shouldShow(s, WA, { x: 10, y: belowTrigger }, true), false, 'off to the side');
});
