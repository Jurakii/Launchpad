const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');
const electron = require('electron'); // the helpers' stub

// nativeImage stand-in: anything ending in an image extension "has" a
// thumbnail, everything else comes back empty like the real shell call does
// for a file type with no thumbnail handler.
const IMAGE_RE = /\.(png|jpe?g|gif|webp|bmp|svg)$/i;
const fakeImage = (bytes) => ({ isEmpty: () => !bytes, toPNG: () => Buffer.from(bytes || '') });
electron.nativeImage = {
  createThumbnailFromPath: async (p) => (IMAGE_RE.test(p) ? fakeImage('thumb') : fakeImage(null)),
};

function setup() {
  const dir = freshUserData();
  electron.app.getFileIcon = async () => fakeImage('icon');
  const store = freshRequire('../lib/store');
  const fileStore = freshRequire('../lib/file-store');
  const src = path.join(dir, 'source');
  fs.mkdirSync(src);
  return { dir, src, store, fileStore, filesDir: path.join(dir, 'files') };
}

function write(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, data);
  return file;
}

test('copying a file keeps its bytes and leaves the original alone', async () => {
  const { src, fileStore, filesDir } = setup();
  const original = write(path.join(src, 'report.pdf'), 'pdf bytes');
  const before = fs.statSync(original).mtimeMs;

  const stored = await fileStore.storeFromPath(original);
  assert.strictEqual(stored.target, path.join(filesDir, stored.id, 'report.pdf'));
  assert.strictEqual(fs.readFileSync(stored.target, 'utf-8'), 'pdf bytes');
  assert.strictEqual(stored.storedKind, 'file');
  assert.strictEqual(stored.size, 9);
  assert.strictEqual(stored.originalPath, original);
  assert.strictEqual(fs.readFileSync(original, 'utf-8'), 'pdf bytes');
  assert.strictEqual(fs.statSync(original).mtimeMs, before);

  // Still opens after the original is deleted.
  fs.rmSync(original);
  assert.ok(fs.existsSync(stored.target));
});

test('copying a folder brings nested files and skips links out of it', async () => {
  const { dir, src, fileStore } = setup();
  const folder = path.join(src, 'Photos');
  write(path.join(folder, 'a.txt'), 'a');
  write(path.join(folder, 'nested', 'deeper', 'b.txt'), 'bb');
  const outside = path.join(dir, 'outside');
  write(path.join(outside, 'secret.txt'), 'nope');
  fs.symlinkSync(outside, path.join(folder, 'link'), 'junction'); // no admin needed, unlike a symlink

  const stored = await fileStore.storeFromPath(folder);
  assert.strictEqual(stored.storedKind, 'folder');
  assert.strictEqual(stored.size, 3, 'the junction is not counted either');
  assert.deepStrictEqual(listDir(stored.target), ['a.txt', 'nested']);
  assert.strictEqual(fs.readFileSync(path.join(stored.target, 'nested', 'deeper', 'b.txt'), 'utf-8'), 'bb');
});

test('a folder over the size or file-count limit is rejected and nothing is written', async () => {
  const { src, fileStore, filesDir } = setup();
  const folder = path.join(src, 'Big');
  for (let i = 0; i < 5; i++) write(path.join(folder, `${i}.bin`), '0123456789');

  await assert.rejects(fileStore.storeFromPath(folder, { limit: 25 }), (err) => err.code === 'tooLarge');
  await assert.rejects(fileStore.storeFromPath(folder, { fileLimit: 3 }), (err) => err.code === 'tooLarge');
  await assert.rejects(fileStore.storeFromPath(path.join(folder, '0.bin'), { limit: 5 }), (err) => err.code === 'tooLarge');
  assert.ok(!fs.existsSync(filesDir) || listDir(filesDir).length === 0);
});

test('a copy that fails part way removes its half-written folder', async () => {
  const { src, fileStore, filesDir } = setup();
  const folder = path.join(src, 'Docs');
  write(path.join(folder, 'one.txt'), '1');
  write(path.join(folder, 'two.txt'), '2');

  const realCopy = fs.promises.copyFile;
  let calls = 0;
  fs.promises.copyFile = async (...args) => {
    if (++calls === 2) throw Object.assign(new Error('disk full'), { code: 'ENOSPC' });
    return realCopy(...args);
  };
  try {
    await assert.rejects(fileStore.storeFromPath(folder), /disk full/);
  } finally {
    fs.promises.copyFile = realCopy;
  }
  assert.deepStrictEqual(listDir(filesDir), []);
  assert.deepStrictEqual(listDir(folder), ['one.txt', 'two.txt'], 'source untouched');
});

test('removeStored only deletes inside the store, and only for UUID ids', async () => {
  const { dir, src, fileStore, filesDir } = setup();
  const stored = await fileStore.storeFromPath(write(path.join(src, 'pic.png'), 'png'));
  const keep = write(path.join(dir, 'keep.txt'), 'keep');

  for (const bad of ['..', '../keep.txt', '..\\source', '', 'thumbs', null, `${stored.id}\\..\\..`]) {
    await assert.rejects(fileStore.removeStored(bad), /Not a stored item id/);
  }
  assert.ok(fs.existsSync(keep));
  assert.ok(fs.existsSync(path.join(src, 'pic.png')));
  assert.ok(fs.existsSync(stored.target));

  await fileStore.removeStored(stored.id);
  assert.ok(!fs.existsSync(path.join(filesDir, stored.id)));
  assert.ok(!fs.existsSync(path.join(filesDir, 'thumbs', `${stored.id}.png`)));
  assert.ok(fs.existsSync(path.join(src, 'pic.png')), 'the original is never touched');
});

test('thumbnails: images are ready, folders generic, other files fall back to the shell icon', async () => {
  const { src, fileStore, filesDir } = setup();
  const thumb = (id) => path.join(filesDir, 'thumbs', `${id}.png`);

  const img = await fileStore.storeFromPath(write(path.join(src, 'cat.jpg'), 'jpg'));
  assert.strictEqual(img.thumbState, 'ready');
  assert.strictEqual(fs.readFileSync(thumb(img.id), 'utf-8'), 'thumb');

  const doc = await fileStore.storeFromPath(write(path.join(src, 'notes.xyz'), 'x'));
  assert.strictEqual(doc.thumbState, 'generic');
  assert.strictEqual(fs.readFileSync(thumb(doc.id), 'utf-8'), 'icon');

  fs.mkdirSync(path.join(src, 'Folder'));
  const folder = await fileStore.storeFromPath(path.join(src, 'Folder'));
  assert.strictEqual(folder.thumbState, 'generic');
  assert.ok(!fs.existsSync(thumb(folder.id)), 'folders use the renderer glyph');

  // No icon either: still generic, and no file written.
  electron.app.getFileIcon = async () => {
    throw new Error('no icon');
  };
  const bare = await fileStore.storeFromPath(write(path.join(src, 'bare.qqq'), 'q'));
  assert.strictEqual(bare.thumbState, 'generic');
  assert.ok(!fs.existsSync(thumb(bare.id)));
});

test('storeMany with one good and one bad path adds one entry and reports one failure', async () => {
  const { src, fileStore, store } = setup();
  const good = write(path.join(src, 'song.mp3'), 'mp3');
  const bad = path.join(src, 'gone.txt');
  const apps = [{ id: store.newId(), kind: 'app', name: 'Existing', parentId: null, gridX: 2, gridY: 1 }];

  const results = await fileStore.storeMany(apps, [bad, good], { gridX: 2, gridY: 1 });
  assert.deepStrictEqual(results.map((r) => r.ok), [false, true]);
  assert.match(results[0].message, /"gone\.txt" could not be read/);
  assert.strictEqual(apps.length, 2);

  const entry = apps[1];
  assert.strictEqual(entry.type, 'stored');
  assert.strictEqual(entry.name, 'song');
  assert.strictEqual(entry.id, results[1].id);
  assert.notDeepStrictEqual([entry.gridX, entry.gridY], [2, 1], 'lands next to the occupied cell');
  assert.ok(fs.existsSync(entry.target));
});

test('a second drop of the same file is a second copy named "(2)"', async () => {
  const { src, fileStore } = setup();
  const file = write(path.join(src, 'photo.png'), 'png');
  const apps = [];
  await fileStore.storeMany(apps, [file, file]);
  assert.deepStrictEqual(apps.map((a) => a.name), ['photo', 'photo (2)']);
  assert.notStrictEqual(apps[0].target, apps[1].target);
  assert.ok(apps.every((a) => fs.existsSync(a.target)));
});

test('storeMany puts items in the given island', async () => {
  const { src, fileStore } = setup();
  const apps = [];
  await fileStore.storeMany(apps, [write(path.join(src, 'a.txt'), 'a')], { parentId: 'island-1' });
  assert.strictEqual(apps[0].parentId, 'island-1');
});

test('a stored entry\'s target is re-derived on load, never trusted from apps.json', () => {
  const { dir, store, filesDir } = setup();
  const id = store.newId();
  const base = { id, kind: 'app', name: 'x', type: 'stored', createdAt: 1 };
  fs.writeFileSync(
    path.join(dir, 'apps.json'),
    JSON.stringify([
      { ...base, storedName: 'x.txt', target: 'C:\\Windows\\System32\\cmd.exe', iconPath: 'C:\\evil.png' },
      { ...base, id: store.newId(), storedName: '..\\..\\..\\Windows\\notepad.exe' },
      { ...base, id: '../../escape', storedName: 'x.txt' },
    ])
  );
  const [ok, badName, badId] = store.load();
  assert.strictEqual(ok.target, path.join(filesDir, id, 'x.txt'));
  assert.strictEqual(ok.iconPath, path.join(filesDir, 'thumbs', `${id}.png`));
  assert.strictEqual(ok.thumbState, 'pending');
  assert.strictEqual(badName.target, '');
  assert.strictEqual(badId.target, '');
  assert.strictEqual(badId.iconPath, null);
});

test('programs, scripts and shortcuts are links; everything else is copied', () => {
  const { fileStore } = setup();
  for (const p of ['C:\\a\\app.EXE', 'x.lnk', 'x.url', 'setup.msi', 'run.bat', 'tool.py', 'a.mjs', 'x.ps1']) {
    assert.ok(fileStore.isLinkable(p), p);
  }
  for (const p of ['photo.png', 'report.pdf', 'archive.zip', 'C:\\Users\\me\\Folder', 'C:\\dir.exe\\file']) {
    assert.ok(!fileStore.isLinkable(p), p);
  }
});

test('files under a path longer than 260 characters copy', { skip: process.platform !== 'win32' }, async () => {
  const { src, fileStore } = setup();
  let deep = path.join(src, 'Long');
  while (deep.length < 300) deep = path.join(deep, 'a-fairly-long-folder-name-for-testing');
  write(path.join(deep, 'deep.txt'), 'deep');

  const stored = await fileStore.storeFromPath(path.join(src, 'Long'));
  const copied = path.join(stored.target, path.relative(path.join(src, 'Long'), deep), 'deep.txt');
  assert.ok(copied.length > 260);
  assert.strictEqual(fs.readFileSync(copied, 'utf-8'), 'deep');
  assert.strictEqual(stored.size, 4);
});
