const test = require('node:test');
const assert = require('node:assert');
const { freshUserData, freshRequire } = require('./helpers');

function setup() {
  freshUserData();
  freshRequire('../lib/settings');
  return freshRequire('../lib/media/controller');
}

const PAD = 'Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 02fd)';

test('defaults when nothing is saved', () => {
  const controller = setup();
  assert.deepStrictEqual(controller.load(), { deadZone: 0.35, pads: {} });
});

test('the dead zone is clamped to 10-90% in 5% steps', () => {
  const controller = setup();
  assert.strictEqual(controller.sanitize({ deadZone: 0.02 }).deadZone, 0.1);
  assert.strictEqual(controller.sanitize({ deadZone: 2 }).deadZone, 0.9);
  assert.strictEqual(controller.sanitize({ deadZone: 0.42 }).deadZone, 0.4);
  assert.strictEqual(controller.sanitize({ deadZone: 'x' }).deadZone, 0.35);
});

test('mappings keep valid buttons and actions, one button per action', () => {
  const controller = setup();
  const { pads } = controller.sanitize({
    pads: { [PAD]: { 1: 'select', 0: 'back', 2: 'select', 40: 'menu', '-1': 'menu', 3: 'fly', x: 'up', 12: 'up' } },
  });
  assert.deepStrictEqual(pads[PAD], { 0: 'back', 1: 'select', 12: 'up' });
});

test('a pad map without select or back is dropped, so the pad keeps working', () => {
  const controller = setup();
  const { pads } = controller.sanitize({ pads: { [PAD]: { 0: 'select', 2: 'menu' }, '': { 0: 'select', 1: 'back' } } });
  assert.deepStrictEqual(pads, {});
});

test('save round-trips through settings.json', () => {
  const controller = setup();
  controller.save({ deadZone: 0.5, pads: { [PAD]: { 1: 'select', 0: 'back' } } });
  assert.deepStrictEqual(controller.load(), { deadZone: 0.5, pads: { [PAD]: { 0: 'back', 1: 'select' } } });
});
