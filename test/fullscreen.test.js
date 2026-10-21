const test = require('node:test');
const assert = require('node:assert');
const fullscreen = require('../lib/fullscreen');

const PRIMARY = { x: 0, y: 0, width: 2560, height: 1440 };
const SECOND = { x: 2560, y: 0, width: 1920, height: 1080 };
const game = { hwnd: 1, covers: true, zoomed: false, own: false, foreground: true };
const reply = (...entries) => ({ ok: true, monitors: entries });
const at = (r, window) => ({ monitor: [r.x, r.y, r.x + r.width, r.y + r.height], window });

test('a covering window of the foreground app takes the monitor over', () => {
  assert.strictEqual(fullscreen.takenOver(game), true);
});

test('a covering window left behind the foreground (a launcher, an overlay) does not', () => {
  assert.strictEqual(fullscreen.takenOver({ ...game, foreground: false }), false);
});

test("Explorer's fullscreen notice counts a covering window that isn't in front", () => {
  assert.strictEqual(fullscreen.takenOver({ ...game, foreground: false }, { explorerHint: true }), true);
});

test('maximized, own, partly covering or no window never takes it over', () => {
  for (const win of [{ ...game, zoomed: true }, { ...game, own: true }, { ...game, covers: false }, null]) {
    assert.strictEqual(fullscreen.takenOver(win, { explorerHint: true }), false);
  }
});

test('each display is matched to its own monitor in the reply', () => {
  const r = reply(at(PRIMARY, null), at(SECOND, game));
  assert.strictEqual(fullscreen.decide(r, PRIMARY, true), false);
  assert.strictEqual(fullscreen.decide(r, SECOND, false), true);
});

test('a display missing from the reply is not fullscreen', () => {
  assert.strictEqual(fullscreen.decide(reply(at(SECOND, game)), PRIMARY, true), false);
});

test('a failed reply keeps the previous state', () => {
  for (const r of [{ ok: false, code: 'badRequest' }, null, { ok: true }]) {
    assert.strictEqual(fullscreen.decide(r, PRIMARY, true), true);
    assert.strictEqual(fullscreen.decide(r, PRIMARY, false), false);
  }
});
