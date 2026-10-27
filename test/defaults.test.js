const test = require('node:test');
const assert = require('node:assert');
const defaults = require('../lib/defaults');

let n = 0;
const newId = () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}`;

test('first run: File Explorer is at the first cell and the Recycle Bin beside it, Explorer the older', () => {
  const [explorer, bin] = defaults.buildEntries(newId, 1000);
  assert.strictEqual(explorer.name, 'File Explorer');
  assert.deepStrictEqual([explorer.gridX, explorer.gridY], [0, 0]);
  assert.strictEqual(bin.name, 'Recycle Bin');
  assert.deepStrictEqual([bin.gridX, bin.gridY], [1, 0]);
  assert.ok(explorer.createdAt < bin.createdAt);
  assert.strictEqual(bin.special, 'recycleBin');
  assert.strictEqual(explorer.special, undefined);
});

test('repair fixes a broken Explorer entry found by its key, keeping name, island, position, pin and colour', () => {
  const [explorer, bin] = defaults.buildEntries(newId);
  Object.assign(explorer, {
    name: 'Files',
    target: 'C:\\nowhere.exe',
    type: 'url',
    parentId: 'island-1',
    gridX: 4,
    gridY: 2,
    pinned: true,
    color: '#f00',
  });
  const apps = [explorer, bin];
  const { repaired, added } = defaults.repair(apps, newId);
  assert.strictEqual(added.length, 0);
  assert.strictEqual(repaired.length, 2);
  assert.strictEqual(explorer.target, defaults.EXPLORER_PATH);
  assert.strictEqual(explorer.type, 'path');
  assert.deepStrictEqual(
    [explorer.name, explorer.parentId, explorer.gridX, explorer.gridY, explorer.pinned, explorer.color],
    ['Files', 'island-1', 4, 2, true, '#f00']
  );
});

test('repair recognises entries from before stockKey, renamed ones included, and adds missing defaults on free cells', () => {
  const apps = [
    { id: newId(), kind: 'app', name: 'My Explorer', type: 'path', target: 'c:\\windows\\EXPLORER.EXE', parentId: null, gridX: 1, gridY: 0 },
    { id: newId(), kind: 'app', name: 'Notes', type: 'path', target: 'C:\\notes.exe', parentId: null, gridX: 0, gridY: 0 },
  ];
  const { repaired, added } = defaults.repair(apps, newId);
  assert.deepStrictEqual(repaired.map((e) => e.name), ['My Explorer']);
  assert.strictEqual(repaired[0].stockKey, 'explorer');
  assert.deepStrictEqual(added.map((e) => e.name), ['Recycle Bin']);
  const bin = added[0];
  assert.ok(!apps.some((a) => a !== bin && a.gridX === bin.gridX && a.gridY === bin.gridY));
  assert.strictEqual(apps.find((a) => a.name === 'Notes').target, 'C:\\notes.exe'); // user apps untouched
});
