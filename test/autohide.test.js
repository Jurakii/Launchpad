const test = require('node:test');
const assert = require('node:assert');
const autohide = require('../lib/autohide');

const BOUNDS = { x: -1920, y: 0, width: 1920, height: 1080 }; // a display left of the primary
const PANEL = { x: -1920, y: 0, width: 1920, height: 32 };
const EDGE = { x: -1000, y: 0 };
const BELOW = { x: -1000, y: 400 };

// Feeds cursor samples [ms, point] in order and returns the state after each.
function run(samples, { canReveal = true, from = autohide.initial() } = {}) {
  let state = from;
  return samples.map(([now, cursor]) => {
    state = autohide.step(state, { cursor, bounds: BOUNDS, panel: PANEL, now, canReveal });
    return state.revealed;
  });
}

test('resting against the top edge reveals the panel, but only after the delay', () => {
  const d = autohide.REVEAL_DELAY_MS;
  assert.deepStrictEqual(run([[0, EDGE], [d - 50, EDGE], [d, EDGE]]), [false, false, true]);
});

test('passing through the top edge on the way to a click does not reveal it', () => {
  const d = autohide.REVEAL_DELAY_MS;
  assert.deepStrictEqual(run([[0, EDGE], [100, BELOW], [d, EDGE], [d + 100, EDGE]]), [false, false, false, false]);
});

test('the edge of another display, or the edge while blocked, never reveals it', () => {
  const d = autohide.REVEAL_DELAY_MS;
  const primaryTop = { x: 500, y: 0 };
  assert.deepStrictEqual(run([[0, primaryTop], [d * 2, primaryTop]]), [false, false]);
  assert.deepStrictEqual(run([[0, EDGE], [d * 2, EDGE]], { canReveal: false }), [false, false]);
});

test('once shown it stays while the cursor is on or just below it, and goes after the delay away', () => {
  const h = autohide.HIDE_DELAY_MS;
  const justBelow = { x: -1000, y: PANEL.height + autohide.STAY_MARGIN_DIP - 1 };
  const from = autohide.initial(true);
  assert.deepStrictEqual(run([[0, { x: -1000, y: 10 }], [1000, justBelow]], { from }), [true, true]);
  assert.deepStrictEqual(run([[0, BELOW], [h - 50, BELOW], [h, BELOW]], { from }), [true, true, false]);
  // Coming back before the delay is up keeps it, and restarts the clock.
  assert.deepStrictEqual(run([[0, BELOW], [h - 50, EDGE], [h + 100, BELOW], [2 * h, BELOW]], { from }), [true, true, true, true]);
});
