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
