const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');

const entry = (name, extra = {}) => ({
  id: `00000000-0000-4000-8000-${String(name.length).padStart(12, '0')}`,
  kind: 'app',
  name,
  createdAt: 1,
  ...extra,
});

function setup() {
  const dir = freshUserData();
  const store = freshRequire('../lib/store');
  const file = path.join(dir, 'apps.json');
  const bak = path.join(dir, 'apps.json.bak');
  return { dir, store, file, bak };
}

const corruptCopies = (dir) => listDir(dir).filter((f) => f.startsWith('apps.corrupt-'));

test('missing apps.json loads as empty and can be saved', () => {
  const { store, file } = setup();
  assert.deepStrictEqual(store.load(), []);
  store.save([entry('A')]);
  assert.strictEqual(JSON.parse(fs.readFileSync(file, 'utf-8'))[0].name, 'A');
  assert.strictEqual(store.getLoadIssue(), null);
});

test('save keeps the previous version as apps.json.bak and leaves no temp file', () => {
  const { dir, store, bak } = setup();
  store.save([entry('A')]);
  store.save([entry('A'), entry('BB')]);
  assert.strictEqual(JSON.parse(fs.readFileSync(bak, 'utf-8')).length, 1);
  assert.ok(!listDir(dir).some((f) => f.endsWith('.tmp')));
});

test('corrupt apps.json with a good backup is restored and the bad file kept', () => {
  const { dir, store, file, bak } = setup();
  fs.writeFileSync(bak, JSON.stringify([entry('A')]));
  fs.writeFileSync(file, '[{"id": "trunc');

  const apps = store.load();
  assert.deepStrictEqual(apps.map((a) => a.name), ['A']);
  assert.strictEqual(store.getLoadIssue().state, 'restored');

  const [copy] = corruptCopies(dir);
  assert.strictEqual(fs.readFileSync(path.join(dir, copy), 'utf-8'), '[{"id": "trunc');
  assert.strictEqual(store.getLoadIssue().corruptCopy, path.join(dir, copy));
  // apps.json itself is valid again, and saving works.
  assert.strictEqual(JSON.parse(fs.readFileSync(file, 'utf-8'))[0].name, 'A');
  store.save([entry('A'), entry('BB')]);
});

test('corrupt apps.json with no backup goes read-only and is never overwritten', () => {
  const { dir, store, file } = setup();
  fs.writeFileSync(file, 'not json');

  assert.deepStrictEqual(store.load(), []);
  assert.strictEqual(store.getLoadIssue().state, 'readOnly');
  assert.throws(() => store.save([entry('A')]), /can't save changes/);
  store.load();
  store.load();
  assert.strictEqual(fs.readFileSync(file, 'utf-8'), 'not json');
  assert.strictEqual(corruptCopies(dir).length, 1, 'only copied aside once');
});

test('a JSON value that is not an array counts as corrupt', () => {
  const { store, file } = setup();
  fs.writeFileSync(file, '{"apps": []}');
  store.load();
  assert.strictEqual(store.getLoadIssue().state, 'readOnly');
});

test('a corrupt backup is treated as no backup', () => {
  const { store, file, bak } = setup();
  fs.writeFileSync(file, 'not json');
  fs.writeFileSync(bak, 'also not json');
  store.load();
  assert.strictEqual(store.getLoadIssue().state, 'readOnly');
});

test('read-only clears once apps.json is fixed on disk', () => {
  const { store, file } = setup();
  fs.writeFileSync(file, 'not json');
  store.load();
  fs.writeFileSync(file, JSON.stringify([]));
  assert.deepStrictEqual(store.load(), []);
  assert.strictEqual(store.getLoadIssue(), null);
  store.save([entry('A')]);
});

test('replaceAll (backup import) works in read-only mode without touching the old backup', () => {
  const { store, file, bak } = setup();
  fs.writeFileSync(file, 'not json');
  fs.writeFileSync(bak, 'old backup');
  store.load();

  store.replaceAll([entry('A')]);
  assert.strictEqual(store.getLoadIssue(), null);
  assert.strictEqual(JSON.parse(fs.readFileSync(file, 'utf-8'))[0].name, 'A');
  assert.strictEqual(fs.readFileSync(bak, 'utf-8'), 'old backup', 'a corrupt file never replaces the backup');
  store.save([entry('A'), entry('BB')]);
});

test('non-object array elements are dropped instead of crashing', () => {
  const { store, file } = setup();
  fs.writeFileSync(file, JSON.stringify([null, 5, entry('A')]));
  assert.deepStrictEqual(store.load().map((a) => a.name), ['A']);
});

test('loads backfill positions for old entries', () => {
  const { store, file } = setup();
  fs.writeFileSync(file, JSON.stringify([entry('A')]));
  const [a] = store.load();
  assert.strictEqual(typeof a.gridX, 'number');
  assert.strictEqual(JSON.parse(fs.readFileSync(file, 'utf-8'))[0].gridX, a.gridX);
});
