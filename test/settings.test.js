const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');

function setup() {
  const dir = freshUserData();
  const settings = freshRequire('../lib/settings');
  return { dir, settings, file: path.join(dir, 'settings.json') };
}

test('save merges into the existing settings', () => {
  const { settings } = setup();
  settings.save({ hotkey: 'Alt+Space' });
  settings.save({ startFullscreen: true });
  assert.deepStrictEqual(settings.load(), { hotkey: 'Alt+Space', startFullscreen: true });
});

test('a corrupt settings.json is moved aside, not silently overwritten', () => {
  const { dir, settings, file } = setup();
  fs.writeFileSync(file, '{"hotkey": "Alt+Sp');

  assert.deepStrictEqual(settings.load(), {});
  const copies = listDir(dir).filter((f) => f.startsWith('settings.corrupt-'));
  assert.strictEqual(copies.length, 1);
  assert.strictEqual(fs.readFileSync(path.join(dir, copies[0]), 'utf-8'), '{"hotkey": "Alt+Sp');

  settings.save({ startFullscreen: true });
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(file, 'utf-8')), { startFullscreen: true });
});

test('organizerOptions: off until enabled, then panel always shown, drag snapping on and Ctrl+Alt+G for the grid', () => {
  const { settings } = setup();
  assert.deepStrictEqual(settings.organizerOptions(), {
    enabled: false,
    panel: 'always',
    snapOnDrag: true,
    gridHotkey: 'CommandOrControl+Alt+G',
  });
  settings.save({ organizerEnabled: true, organizerPanel: 'autoHide', organizerSnapOnDrag: false, organizerHotkey: 'Alt+F1' });
  assert.deepStrictEqual(settings.organizerOptions(), { enabled: true, panel: 'autoHide', snapOnDrag: false, gridHotkey: 'Alt+F1' });
  assert.strictEqual(settings.organizerOptions({ organizerPanel: 'sideways' }).panel, 'always', 'unknown modes fall back');
  assert.strictEqual(settings.organizerOptions({ organizerHotkey: '' }).gridHotkey, 'CommandOrControl+Alt+G');
});
