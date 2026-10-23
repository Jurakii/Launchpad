const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire } = require('./helpers');

const ID = '11111111-1111-4111-8111-111111111111';
const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
const JPEG = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]);

// nativeImage stand-in: reports a size and records the resize it was asked for.
function fakeNativeImage(size) {
  const calls = [];
  const image = (s) => ({
    isEmpty: () => false,
    getSize: () => s,
    resize: (opts) => {
      calls.push(opts);
      return image({ width: opts.width, height: opts.height });
    },
    toPNG: () => Buffer.from(`png ${s.width}x${s.height}`),
    toJPEG: () => Buffer.from(`jpeg ${s.width}x${s.height}`),
  });
  return { calls, createFromBuffer: () => image(size) };
}

function setup(size = { width: 3000, height: 1500 }) {
  const dir = freshUserData();
  const fake = fakeNativeImage(size);
  require('electron').nativeImage = fake;
  freshRequire('../lib/media/library');
  const art = freshRequire('../lib/media/art');
  return { dir, art, fake };
}

test('fitSize shrinks to fit and never enlarges', () => {
  const { art } = setup();
  assert.deepStrictEqual(art.fitSize(3000, 1500, 1920, 1080), { width: 1920, height: 960 });
  assert.deepStrictEqual(art.fitSize(1000, 2000, 600, 900), { width: 450, height: 900 });
  assert.deepStrictEqual(art.fitSize(300, 200, 600, 900), { width: 300, height: 200 });
});

test('saveArt resizes into the slot box and writes slot-named files', () => {
  const { art, fake, dir } = setup();
  const value = art.saveArt(ID, 'banner', PNG, 'user', 99);
  assert.deepStrictEqual(value, { path: 'banner.jpg', source: 'user', updatedAt: 99 });
  assert.deepStrictEqual(fake.calls[0], { width: 1920, height: 960, quality: 'best' });
  const file = path.join(dir, 'media', ID, 'banner.jpg');
  assert.strictEqual(fs.readFileSync(file, 'utf-8'), 'jpeg 1920x960');
  assert.ok(!fs.existsSync(`${file}.tmp`));

  art.saveArt(ID, 'logo', JPEG, 'user');
  assert.strictEqual(fs.readFileSync(path.join(dir, 'media', ID, 'logo.png'), 'utf-8'), 'png 800x400', 'logos stay PNG');
});

test('saveArt refuses unknown slots, bad ids and non-PNG/JPEG data', () => {
  const { art } = setup();
  assert.throws(() => art.saveArt(ID, 'poster', PNG, 'user'), /Unknown art slot/);
  assert.throws(() => art.saveArt('..', 'cover', PNG, 'user'), /Invalid library id/);
  assert.throws(() => art.saveArt(ID, 'cover', Buffer.from('GIF89a'), 'user'), /PNG or JPEG/);
});

test('clearArt removes the file, and existingArt only lists files that exist', () => {
  const { art } = setup();
  const cover = art.saveArt(ID, 'cover', PNG, 'user');
  const entry = { id: ID, media: { cover, banner: { path: 'banner.jpg', source: 'user' }, logo: null } };
  assert.deepStrictEqual(Object.keys(art.existingArt(entry)), ['cover']);
  art.clearArt(ID, 'cover');
  assert.deepStrictEqual(art.existingArt(entry), {});
  art.clearArt(ID, 'cover'); // already gone: no error
});

test('readImageFile checks the path before reading', () => {
  const { art, dir } = setup();
  const file = path.join(dir, 'pic.png');
  fs.writeFileSync(file, PNG);
  assert.deepStrictEqual(art.readImageFile(file), PNG);
  assert.throws(() => art.readImageFile('pic.png'), /Choose an image file/);
  assert.throws(() => art.readImageFile(path.join(dir, 'missing.png')), /Can't find/);
  assert.throws(() => art.readImageFile(dir), /Choose an image file/);
});

test('resolveArtUrl only maps lpmedia://art/<uuid>/<slot> into the media folder', () => {
  const { art, dir } = setup();
  const media = path.join(dir, 'media');
  assert.strictEqual(art.resolveArtUrl(`lpmedia://art/${ID}/cover?v=5`), path.join(media, ID, 'cover.jpg'));
  assert.strictEqual(art.resolveArtUrl(`lpmedia://art/${ID.toUpperCase()}/logo`), path.join(media, ID, 'logo.png'));
  for (const bad of [
    `lpmedia://art/${ID}/poster`,
    `lpmedia://art/${ID}`,
    `lpmedia://art/${ID}/cover/extra`,
    `lpmedia://other/${ID}/cover`,
    `lpmedia://art/../${ID}/cover`,
    `lpmedia://art/%2e%2e/cover`,
    `lpmedia://art/not-an-id/cover`,
    `file:///C:/Windows/win.ini`,
    'not a url',
  ]) {
    const resolved = art.resolveArtUrl(bad);
    assert.ok(resolved === null || resolved.startsWith(media), bad);
    if (bad.includes('..') || bad.includes('%2e')) assert.ok(resolved === null || resolved === path.join(media, ID, 'cover.jpg'), bad);
  }
  assert.strictEqual(art.resolveArtUrl(`lpmedia://art/${ID}/poster`), null);
  assert.strictEqual(art.resolveArtUrl('file:///C:/Windows/win.ini'), null);
});
