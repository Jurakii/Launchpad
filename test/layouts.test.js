const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');

const BUILT_IN_IDS = ['halves', 'thirds', 'focus', 'main-stack', 'quarters'];

function setup(fileContents) {
  const dir = freshUserData();
  const file = path.join(dir, 'layouts.json');
  if (fileContents !== undefined) {
    fs.writeFileSync(file, typeof fileContents === 'string' ? fileContents : JSON.stringify(fileContents));
  }
  return { dir, file, layouts: freshRequire('../lib/layouts') };
}

const custom = (id, name = 'Wide center') => ({
  id,
  name,
  cols: 4,
  rows: 1,
  zones: [
    { id: 'left', x: 0, y: 0, w: 1, h: 1 },
    { id: 'center', x: 1, y: 0, w: 2, h: 1 },
    { id: 'right', x: 3, y: 0, w: 1, h: 1 },
  ],
});

test('with no file: the built-ins in order and the default gap, without creating the file', () => {
  const { dir, layouts } = setup();
  const data = layouts.load();
  assert.deepStrictEqual(data.layouts.map((l) => l.id), BUILT_IN_IDS);
  assert.strictEqual(data.gap, layouts.DEFAULT_GAP);
  assert.deepStrictEqual(listDir(dir), []);
});

test("the user's layouts follow the built-ins; invalid ones and built-in overrides are skipped", (t) => {
  t.mock.method(console, 'error', () => {});
  const broken = { ...custom('broken'), zones: [{ id: 'a', x: 0, y: 0, w: 9, h: 1 }] };
  const override = { ...custom('halves'), name: 'Not really halves' };
  const { layouts } = setup({ layouts: [custom('wide'), broken, override] });

  const data = layouts.load();
  assert.deepStrictEqual(data.layouts.map((l) => l.id), [...BUILT_IN_IDS, 'wide']);
  assert.strictEqual(data.layouts.find((l) => l.id === 'halves').name, 'Halves');
  assert.strictEqual(console.error.mock.callCount(), 1, 'the broken layout is reported');
});

test('the gap falls back to the default unless it is a whole number from 0 to 64', () => {
  for (const [gap, expected] of [[0, 0], [12, 12], [64, 64], [65, 8], [-1, 8], [2.5, 8], ['8', 8]]) {
    const { layouts } = setup({ gap });
    assert.strictEqual(layouts.load().gap, expected, `gap ${JSON.stringify(gap)}`);
  }
});

test('a corrupt layouts.json is moved aside and the defaults are used', (t) => {
  t.mock.method(console, 'error', () => {});
  const { dir, layouts } = setup('{"layouts": [');
  assert.deepStrictEqual(layouts.load().layouts.map((l) => l.id), BUILT_IN_IDS);
  const copies = listDir(dir).filter((f) => f.startsWith('layouts.corrupt-'));
  assert.strictEqual(copies.length, 1);
  assert.strictEqual(fs.readFileSync(path.join(dir, copies[0]), 'utf-8'), '{"layouts": [');
});

test('save writes only the user layouts, keeps unknown keys, and round-trips', () => {
  const { file, layouts } = setup({ activeByDisplay: { 123: 'thirds' } });
  const data = layouts.load();
  data.layouts.push(custom('wide'));
  data.gap = 0;
  layouts.save(data);

  const written = JSON.parse(fs.readFileSync(file, 'utf-8'));
  assert.deepStrictEqual(written.layouts.map((l) => l.id), ['wide']);
  assert.deepStrictEqual(written.activeByDisplay, { 123: 'thirds' });

  const reloaded = layouts.load();
  assert.deepStrictEqual(reloaded.layouts.map((l) => l.id), [...BUILT_IN_IDS, 'wide']);
  assert.strictEqual(reloaded.gap, 0);
});

test('load hands out copies, so changing one does not change the built-ins', () => {
  const { layouts } = setup();
  layouts.load().layouts[0].zones[0].w = 99;
  assert.strictEqual(layouts.load().layouts[0].zones[0].w, 1);
});

test('a second layout with an id already in use is skipped', () => {
  const { layouts } = setup({ layouts: [custom('wide'), custom('wide', 'Another')] });
  const wide = layouts.load().layouts.filter((l) => l.id === 'wide');
  assert.deepStrictEqual(wide.map((l) => l.name), ['Wide center']);
});

test('saveLayout adds a layout, replaces it by id, and refuses built-ins and broken ones', () => {
  const { file, layouts } = setup();
  layouts.saveLayout(custom('wide'));
  layouts.saveLayout({ ...custom('wide'), name: '  Renamed  ' });
  const data = layouts.load();
  assert.deepStrictEqual(data.layouts.map((l) => l.id), [...BUILT_IN_IDS, 'wide']);
  assert.strictEqual(data.layouts.at(-1).name, 'Renamed');
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(file, 'utf-8')).layouts.map((l) => l.id), ['wide']);

  assert.throws(() => layouts.saveLayout({ ...custom('halves') }), /Built-in/);
  assert.throws(() => layouts.saveLayout({ ...custom('bad'), zones: [{ id: 'a', x: 0, y: 0, w: 2, h: 1 }, { id: 'b', x: 1, y: 0, w: 1, h: 1 }] }), /overlap/);
  assert.throws(() => layouts.saveLayout({ ...custom('bad'), name: '   ' }), /missing name/);
  assert.strictEqual(layouts.load().layouts.length, BUILT_IN_IDS.length + 1, 'nothing was saved by the failures');
});

test('saveLayout throws rather than overwriting a file it cannot read', (t) => {
  const { file, layouts } = setup({ layouts: [custom('wide')] });
  const readFileSync = fs.readFileSync;
  t.mock.method(fs, 'readFileSync', (p, ...rest) => {
    if (p === file) throw Object.assign(new Error('locked'), { code: 'EBUSY' });
    return readFileSync.call(fs, p, ...rest);
  });
  assert.throws(() => layouts.saveLayout(custom('other')), /locked/);
  t.mock.restoreAll();
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(file, 'utf-8')).layouts.map((l) => l.id), ['wide']);
});

test('removeLayout deletes only user layouts, and forgets that they were hidden', () => {
  const { layouts } = setup({ layouts: [custom('wide')], hidden: ['wide', 'thirds'] });
  assert.throws(() => layouts.removeLayout('halves'), /only hidden/);
  const data = layouts.removeLayout('wide');
  assert.deepStrictEqual(data.layouts.map((l) => l.id), BUILT_IN_IDS);
  assert.deepStrictEqual(data.hidden, ['thirds']);
});

test('hidden layouts are left out of visible(), but one always stays shown', () => {
  const { layouts } = setup({ layouts: [custom('wide')] });
  layouts.setHidden('thirds', true);
  layouts.setHidden('wide', true);
  let data = layouts.load();
  assert.deepStrictEqual(layouts.visible(data).map((l) => l.id), ['halves', 'focus', 'main-stack', 'quarters']);
  assert.deepStrictEqual(layouts.library(data).layouts.filter((l) => l.hidden).map((l) => l.id), ['thirds', 'wide']);
  assert.deepStrictEqual(layouts.library(data).layouts.filter((l) => l.builtIn).map((l) => l.id), BUILT_IN_IDS);

  layouts.setHidden('wide', false);
  for (const id of ['halves', 'focus', 'main-stack', 'quarters']) layouts.setHidden(id, true);
  assert.throws(() => layouts.setHidden('wide', true), /stay shown/);
  assert.throws(() => layouts.setHidden('nope', true), /no longer exists/);

  data = layouts.load();
  assert.deepStrictEqual(layouts.visible(data).map((l) => l.id), ['wide']);
  assert.strictEqual(layouts.visible({ ...data, hidden: data.layouts.map((l) => l.id) }).length, data.layouts.length, 'all hidden by hand: all shown');
});

test('hidden ids that are not layouts are dropped', () => {
  const { layouts } = setup({ hidden: ['thirds', 'gone', 42, 'thirds'] });
  assert.deepStrictEqual(layouts.load().hidden, ['thirds']);
});

test('setGap accepts whole numbers from 0 to 64', () => {
  const { layouts } = setup();
  assert.strictEqual(layouts.setGap(0).gap, 0);
  assert.strictEqual(layouts.load().gap, 0);
  for (const bad of [-1, 65, 2.5, '8']) assert.throws(() => layouts.setGap(bad), /whole number/);
});

test('exportData and importData round-trip the user layouts, hidden list and gap', (t) => {
  t.mock.method(console, 'error', () => {});
  const first = setup({ layouts: [custom('wide')], hidden: ['thirds'], gap: 4 });
  const backup = JSON.parse(JSON.stringify(first.layouts.exportData()));
  assert.deepStrictEqual(backup.layouts.map((l) => l.id), ['wide'], 'no built-ins in the backup');

  const second = setup({ layouts: [custom('other')] });
  const data = second.layouts.importData({ ...backup, layouts: [...backup.layouts, { id: 'broken' }] });
  assert.deepStrictEqual(data.layouts.map((l) => l.id), [...BUILT_IN_IDS, 'wide'], 'replaced, and the broken one dropped');
  assert.deepStrictEqual(data.hidden, ['thirds']);
  assert.strictEqual(data.gap, 4);
  assert.deepStrictEqual(second.layouts.importData(null).layouts.map((l) => l.id), BUILT_IN_IDS);
});

const savedScene = (id, name = 'Coding', islandId = null) => ({
  id,
  name,
  islandId,
  savedAt: 1,
  windows: [
    {
      exe: 'C:\Apps\Code.exe',
      title: 'main.js',
      display: { id: 1, bounds: { x: 0, y: 0, width: 1920, height: 1080 } },
      rect: { x: 0, y: 0, w: 0.5, h: 1 },
      maximized: false,
    },
  ],
});

test('scenes: saveScene adds and replaces by id, renameScene and removeScene change the file', () => {
  const { file, layouts } = setup();
  layouts.saveScene(savedScene('a'));
  layouts.saveScene(savedScene('b', 'Writing', 'island-1'));
  layouts.saveScene({ ...savedScene('a'), name: '  Coding 2  ' });
  assert.deepStrictEqual(layouts.load().scenes.map((s) => [s.id, s.name]), [['a', 'Coding 2'], ['b', 'Writing']]);

  layouts.renameScene('b', 'Reading');
  layouts.removeScene('a');
  const onDisk = JSON.parse(fs.readFileSync(file, 'utf-8'));
  assert.deepStrictEqual(onDisk.scenes.map((s) => [s.id, s.name, s.islandId]), [['b', 'Reading', 'island-1']]);
});

test('scenes: broken ones are refused when saved and skipped when read', (t) => {
  t.mock.method(console, 'error', () => {});
  const { layouts } = setup({ scenes: [savedScene('ok'), { id: 'broken', windows: [] }] });
  assert.deepStrictEqual(layouts.load().scenes.map((s) => s.id), ['ok']);
  assert.throws(() => layouts.saveScene({ ...savedScene('x'), windows: [] }), /can't be saved: it has no windows/);
  assert.throws(() => layouts.renameScene('ok', '   '), /needs a name/);
  assert.throws(() => layouts.renameScene('gone', 'Name'), /no longer exists/);
});

test('scenes: saving layouts keeps the scenes, and backups carry them', () => {
  const first = setup({ scenes: [savedScene('a')] });
  first.layouts.setGap(12);
  assert.deepStrictEqual(first.layouts.load().scenes.map((s) => s.id), ['a']);
  const backup = JSON.parse(JSON.stringify(first.layouts.exportData()));

  const second = setup();
  assert.deepStrictEqual(second.layouts.importData(backup).scenes.map((s) => s.id), ['a']);
});
