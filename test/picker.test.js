const test = require('node:test');
const assert = require('node:assert');
const picker = require('../lib/picker');

const fg = (kind, snappable) => ({ ev: 'foreground', hwnd: 10, kind, visible: true, title: 't', exe: 'a.exe', snappable });

test('panelAction snaps a snappable window straight away', () => {
  assert.strictEqual(picker.panelAction(fg('app', true), true), 'snap');
});

test('panelAction opens the picker when there is nothing to move', () => {
  assert.strictEqual(picker.panelAction(fg('own', false), true), 'pick'); // Launchpad in front
  assert.strictEqual(picker.panelAction(fg('shell', false), true), 'pick'); // the desktop
  assert.strictEqual(picker.panelAction(fg('app', false), true), 'pick'); // a fixed-size dialog
  assert.strictEqual(picker.panelAction(null, true), 'pick');
});

test('panelAction waits while the helper is down, whatever is focused', () => {
  assert.strictEqual(picker.panelAction(fg('app', true), false), 'wait');
  assert.strictEqual(picker.panelAction(null, false), 'wait');
});

test('pickerEntries keeps z-order and drops unusable, duplicate and untitled windows', () => {
  const entries = picker.pickerEntries([
    { hwnd: 3, exe: 'C:\\a.exe', title: ' Notes ', frame: [0, 0, 1, 1], minimized: false, maximized: true },
    { hwnd: 0, exe: 'C:\\b.exe', title: 'Zero' },
    { hwnd: 3, exe: 'C:\\a.exe', title: 'Again' },
    { hwnd: 5, exe: 'C:\\c.exe', title: '   ' },
    { hwnd: 'x', title: 'Bad handle' },
    null,
    { hwnd: 7, title: 'Mail', minimized: true },
  ]);
  assert.deepStrictEqual(entries, [
    { hwnd: 3, exe: 'C:\\a.exe', title: 'Notes', minimized: false },
    { hwnd: 7, exe: '', title: 'Mail', minimized: true },
  ]);
  assert.deepStrictEqual(picker.pickerEntries(undefined), []);
});

test('pickerEntries caps the list', () => {
  const many = Array.from({ length: picker.MAX_ENTRIES + 5 }, (_, i) => ({ hwnd: i + 1, exe: 'a.exe', title: `W${i}` }));
  const entries = picker.pickerEntries(many);
  assert.strictEqual(entries.length, picker.MAX_ENTRIES);
  assert.strictEqual(entries[0].title, 'W0');
});

test('findEntry only finds windows the picker was shown', () => {
  const entries = picker.pickerEntries([{ hwnd: 3, exe: 'a.exe', title: 'Notes' }]);
  assert.strictEqual(picker.findEntry(entries, 3).title, 'Notes');
  assert.strictEqual(picker.findEntry(entries, 4), null);
  assert.strictEqual(picker.findEntry(null, 3), null);
});
