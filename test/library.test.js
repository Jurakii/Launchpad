const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');

const ID_A = '11111111-1111-4111-8111-111111111111';
const ID_B = '22222222-2222-4222-8222-222222222222';

const entry = (name, extra = {}) => ({
  id: ID_A,
  library: 'games',
  name,
  platform: 'snes',
  type: 'profile',
  target: `C:\\ROMs\\snes\\${name}.sfc`,
  ...extra,
});

function setup() {
  const dir = freshUserData();
  const library = freshRequire('../lib/media/library');
  const file = path.join(dir, 'library.json');
  const bak = path.join(dir, 'library.json.bak');
  return { dir, library, file, bak };
}

const corruptCopies = (dir) => listDir(dir).filter((f) => f.startsWith('library.corrupt-'));
const onDisk = (file) => JSON.parse(fs.readFileSync(file, 'utf-8'));

test('missing library.json loads as empty and can be saved', () => {
  const { library, file } = setup();
  assert.deepStrictEqual(library.load(), []);
  library.save([entry('A')]);
  assert.strictEqual(onDisk(file)[0].name, 'A');
  assert.strictEqual(library.getLoadIssue(), null);
});

test('save keeps the previous version as library.json.bak and leaves no temp file', () => {
  const { dir, library, bak } = setup();
  library.save([entry('A')]);
  library.save([entry('A'), entry('B', { id: ID_B })]);
  assert.strictEqual(onDisk(bak).length, 1);
  assert.ok(!listDir(dir).some((f) => f.endsWith('.tmp')));
});

test('library.json and apps.json are separate files', () => {
  const { dir, library } = setup();
  library.save([entry('A')]);
  assert.ok(!listDir(dir).includes('apps.json'));
});

test('corrupt library.json with a good backup is restored and the bad file kept', () => {
  const { dir, library, file, bak } = setup();
  fs.writeFileSync(bak, JSON.stringify([entry('A')]));
  fs.writeFileSync(file, '[{"id": "trunc');

  assert.deepStrictEqual(library.load().map((e) => e.name), ['A']);
  assert.strictEqual(library.getLoadIssue().state, 'restored');
  const [copy] = corruptCopies(dir);
  assert.strictEqual(fs.readFileSync(path.join(dir, copy), 'utf-8'), '[{"id": "trunc');
  assert.strictEqual(onDisk(file)[0].name, 'A');
  library.save([entry('A')]);
});

test('corrupt library.json with no backup goes read-only and is never overwritten', () => {
  const { dir, library, file } = setup();
  fs.writeFileSync(file, 'not json');

  assert.deepStrictEqual(library.load(), []);
  assert.strictEqual(library.getLoadIssue().state, 'readOnly');
  assert.throws(() => library.save([entry('A')]), /can't save library changes/);
  library.load();
  assert.strictEqual(fs.readFileSync(file, 'utf-8'), 'not json');
  assert.strictEqual(corruptCopies(dir).length, 1, 'only copied aside once');
});

test('a JSON value that is not an array counts as corrupt', () => {
  const { library, file } = setup();
  fs.writeFileSync(file, '{"games": []}');
  library.load();
  assert.strictEqual(library.getLoadIssue().state, 'readOnly');
});

test('read-only clears once library.json is fixed on disk', () => {
  const { library, file } = setup();
  fs.writeFileSync(file, 'not json');
  library.load();
  fs.writeFileSync(file, '[]');
  assert.deepStrictEqual(library.load(), []);
  assert.strictEqual(library.getLoadIssue(), null);
  library.save([entry('A')]);
});

test('replaceAll works in read-only mode without touching the old backup', () => {
  const { library, file, bak } = setup();
  fs.writeFileSync(file, 'not json');
  fs.writeFileSync(bak, 'old backup');
  library.load();

  library.replaceAll([entry('A')]);
  assert.strictEqual(library.getLoadIssue(), null);
  assert.strictEqual(onDisk(file)[0].name, 'A');
  assert.strictEqual(fs.readFileSync(bak, 'utf-8'), 'old backup');
  library.save([entry('A')]);
});

test('non-object elements are dropped instead of crashing', () => {
  const { library, file } = setup();
  fs.writeFileSync(file, JSON.stringify([null, 5, [], entry('A')]));
  assert.deepStrictEqual(library.load().map((e) => e.name), ['A']);
});

test('bad and duplicate ids get fresh UUIDs, saved back to disk', () => {
  const { library, file } = setup();
  fs.writeFileSync(
    file,
    JSON.stringify([entry('A'), entry('Dup'), entry('Bad', { id: '..\\..\\evil' }), entry('None', { id: undefined })])
  );
  const entries = library.load();
  const ids = entries.map((e) => e.id);
  assert.strictEqual(ids[0], ID_A, 'a valid first id is kept');
  assert.ok(ids.every((id) => library.isValidId(id)));
  assert.strictEqual(new Set(ids).size, 4);
  assert.deepStrictEqual(onDisk(file).map((e) => e.id), ids);
});

test('normalize fills defaults and drops unknown values', () => {
  const { library } = setup();
  const e = library.normalize({ id: ID_A, library: 'music', type: 'rocket', media: { cover: 'x', banner: { path: 'b.jpg', source: 'user' }, junk: {} }, meta: [] });
  assert.strictEqual(e.library, 'games');
  assert.strictEqual(e.type, 'profile');
  assert.strictEqual(e.name, '');
  assert.strictEqual(e.platform, null);
  assert.strictEqual(e.profile, null);
  assert.strictEqual(e.target, '');
  assert.strictEqual(e.available, true);
  assert.strictEqual(e.launchCount, 0);
  assert.deepStrictEqual(Object.keys(e.media), library.MEDIA_SLOTS);
  assert.strictEqual(e.media.cover, null);
  assert.deepStrictEqual(e.media.banner, { path: 'b.jpg', source: 'user' });
  assert.deepStrictEqual(e.meta, {});
});

test('normalize keeps valid values as they are', () => {
  const { library } = setup();
  const e = library.normalize(entry('A', { library: 'videos', type: 'url', profile: 'mpv', available: false, launchCount: 3, createdAt: 9 }));
  assert.strictEqual(e.library, 'videos');
  assert.strictEqual(e.type, 'url');
  assert.strictEqual(e.profile, 'mpv');
  assert.strictEqual(e.available, false);
  assert.strictEqual(e.launchCount, 3);
  assert.strictEqual(e.createdAt, 9);
});

test('mediaDirFor stays inside the media folder and rejects non-UUID ids', () => {
  const { library } = setup();
  const dir = library.mediaDirFor(ID_A);
  assert.strictEqual(path.dirname(dir), library.mediaRoot());
  for (const bad of ['..', '..\\x', 'C:\\Windows', '', null, `${ID_A}\\..`]) {
    assert.throws(() => library.mediaDirFor(bad), /Invalid library id/);
  }
});

test('singleAppEntry makes a path entry that runs the program itself', () => {
  const { library } = setup();
  const opts = { exists: () => true, platforms: new Set(['snes', 'gamecube']), now: 7 };
  const e = library.singleAppEntry({ target: 'C:\\Games\\Celeste\\Celeste.exe', name: ' Celeste ', library: 'games', platform: '' }, opts);
  assert.ok(library.isValidId(e.id));
  assert.deepStrictEqual(
    [e.type, e.target, e.name, e.library, e.platform, e.profile, e.available, e.createdAt, e.launchCount],
    ['path', 'C:\\Games\\Celeste\\Celeste.exe', 'Celeste', 'games', null, null, true, 7, 0]
  );
  assert.strictEqual(library.singleAppEntry({ target: 'C:\\Apps\\Kodi.exe', library: 'videos', platform: 'snes' }, opts).platform, null);
  assert.strictEqual(library.singleAppEntry({ target: 'C:\\Apps\\Kodi.exe' }, opts).name, 'Kodi');
});

test('singleAppEntry refuses anything but an existing absolute .exe, and unknown libraries or platforms', () => {
  const { library } = setup();
  const opts = { exists: (p) => p !== 'C:\\Gone.exe', platforms: new Set(['snes']) };
  for (const target of ['Game.exe', 'C:\\Games\\run.bat', 'C:\\Games\\run.cmd', 'C:\\Games\\rom.sfc', '', null]) {
    assert.throws(() => library.singleAppEntry({ target }, opts), /full path/, String(target));
  }
  assert.throws(() => library.singleAppEntry({ target: 'C:\\Gone.exe' }, opts), /isn't there/);
  assert.throws(() => library.singleAppEntry({ target: 'C:\\A.exe', library: 'music' }, opts), /games or videos/);
  assert.throws(() => library.singleAppEntry({ target: 'C:\\A.exe', platform: 'atari' }, opts), /Unknown platform/);
});
