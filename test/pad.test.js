const test = require('node:test');
const assert = require('node:assert');
const pad = require('../renderer/media/pad');

// A pad snapshot shaped like a Gamepad: buttons by index, left stick axes.
function snapshot({ buttons = [], axes = [0, 0], triggers = {} } = {}) {
  const list = Array.from({ length: 17 }, (_, i) => ({ pressed: buttons.includes(i), value: buttons.includes(i) ? 1 : 0 }));
  for (const [i, value] of Object.entries(triggers)) list[i] = { pressed: false, value };
  return { connected: true, buttons: list, axes };
}

test('stickDirection: nothing inside the dead zone, the larger axis outside it', () => {
  assert.strictEqual(pad.stickDirection(0.2, 0.2, 0.35), null);
  assert.strictEqual(pad.stickDirection(0.9, 0.1, 0.35), 'right');
  assert.strictEqual(pad.stickDirection(-0.5, 0.2, 0.35), 'left');
  assert.strictEqual(pad.stickDirection(0.3, -0.8, 0.35), 'up');
  assert.strictEqual(pad.stickDirection(0.1, 0.6, 0.35), 'down');
});

test('the default mapping follows the Xbox layout', () => {
  const held = (buttons) => [...pad.heldActions(snapshot({ buttons }), pad.DEFAULT_MAPPING, 0.35)];
  assert.deepStrictEqual(held([0]), ['select']);
  assert.deepStrictEqual(held([1]), ['back']);
  assert.deepStrictEqual(held([4, 5]), ['tabPrev', 'tabNext']);
  assert.deepStrictEqual(held([9]), ['menu']);
  assert.deepStrictEqual(held([12, 15]), ['up', 'right']);
  assert.deepStrictEqual(held([16]), [], 'the Guide button is not mapped');
});

test('analogue triggers count as pressed past half way', () => {
  const held = (value) => [...pad.heldActions(snapshot({ triggers: { 7: value } }), pad.DEFAULT_MAPPING, 0.35)];
  assert.deepStrictEqual(held(0.3), []);
  assert.deepStrictEqual(held(0.8), ['pageDown']);
});

test('a press fires once, not every frame it is held', () => {
  const reader = pad.createPadReader();
  const a = snapshot({ buttons: [0] });
  assert.deepStrictEqual(reader.update([a], 0), ['select']);
  assert.deepStrictEqual(reader.update([a], 16), []);
  assert.deepStrictEqual(reader.update([a], 2000), [], 'select never repeats');
  assert.deepStrictEqual(reader.update([snapshot()], 2016), []);
  assert.deepStrictEqual(reader.update([a], 2032), ['select'], 'fires again after release');
});

test('held movement repeats after the delay, then at the repeat rate', () => {
  const reader = pad.createPadReader();
  const down = snapshot({ buttons: [13] });
  const { REPEAT_DELAY_MS: delay, REPEAT_RATE_MS: rate } = pad;
  assert.deepStrictEqual(reader.update([down], 0), ['down']);
  assert.deepStrictEqual(reader.update([down], delay - 1), []);
  assert.deepStrictEqual(reader.update([down], delay), ['down']);
  assert.deepStrictEqual(reader.update([down], delay + rate - 1), []);
  assert.deepStrictEqual(reader.update([down], delay + rate), ['down']);
});

test('the stick moves like the D-pad and respects a custom dead zone', () => {
  const reader = pad.createPadReader({ deadZone: 0.6 });
  assert.deepStrictEqual(reader.update([snapshot({ axes: [0.5, 0] })], 0), []);
  assert.deepStrictEqual(reader.update([snapshot({ axes: [0.7, 0] })], 16), ['right']);
});

test('reset swallows buttons still held until they are released', () => {
  const reader = pad.createPadReader();
  const a = snapshot({ buttons: [0] });
  reader.reset([a]);
  assert.deepStrictEqual(reader.update([a], 0), []);
  assert.deepStrictEqual(reader.update([a], 5000), []);
  reader.update([snapshot()], 5016);
  assert.deepStrictEqual(reader.update([a], 5032), ['select']);
});

test('two pads act as one, and disconnected pads are ignored', () => {
  const reader = pad.createPadReader();
  const p1 = snapshot({ buttons: [0] });
  const p2 = snapshot({ buttons: [0, 1] });
  assert.deepStrictEqual(reader.update([p1, p2], 0).sort(), ['back', 'select']);
  assert.deepStrictEqual(reader.update([{ ...snapshot({ buttons: [9] }), connected: false }, null], 16), []);
});

test('a custom mapping replaces the default', () => {
  const reader = pad.createPadReader({ mapping: { 1: 'select', 0: 'back' } });
  assert.deepStrictEqual(reader.update([snapshot({ buttons: [1] })], 0), ['select']);
});
