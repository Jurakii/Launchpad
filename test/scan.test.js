const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire } = require('./helpers');

function setup() {
  const dir = freshUserData();
  const scan = freshRequire('../lib/media/scan');
  const root = path.join(dir, 'lib');
  // Creates empty files at the given paths under root.
  const touch = (...rels) => {
    for (const rel of rels) {
      const file = path.join(root, rel);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, '');
    }
  };
  return { scan, root, touch };
}

const names = (found) => found.map((f) => f.name).sort();

test('detectPlatform: nearest folder alias wins, then a unique extension', () => {
  const { scan } = setup();
  assert.strictEqual(scan.detectPlatform(['Nintendo 64'], '.zip'), 'n64');
  assert.strictEqual(scan.detectPlatform(['Super Famicom'], '.sfc'), 'snes');
  assert.strictEqual(scan.detectPlatform(['psx', 'Disc Games'], '.chd'), 'psx');
  assert.strictEqual(scan.detectPlatform(['snes', 'n64'], '.sfc'), 'n64', 'nearest folder beats extension');
  assert.strictEqual(scan.detectPlatform(['misc'], '.SMC'), 'snes');
  assert.strictEqual(scan.detectPlatform(['misc'], '.iso'), null);
  assert.strictEqual(scan.detectPlatform([], '.zip'), null);
});

test('cleanName strips region tags and keeps a year apart', () => {
  const { scan } = setup();
  assert.deepStrictEqual(scan.cleanName('Chrono Trigger (USA) [!].sfc'), { name: 'Chrono Trigger', year: null });
  assert.deepStrictEqual(scan.cleanName('Blade Runner 2049 (2017).mkv'), { name: 'Blade Runner 2049', year: 2017 });
  assert.deepStrictEqual(scan.cleanName('The_Expanse.S02E04.mkv'), { name: 'The Expanse S02E04', year: null });
  assert.deepStrictEqual(scan.cleanName('(USA).sfc'), { name: '(USA)', year: null }, 'never an empty name');
});

test('dropTrackFiles keeps the playlist or cue sheet, not its tracks', () => {
  const { scan } = setup();
  assert.deepStrictEqual(scan.dropTrackFiles(['G.cue', 'G (Track 1).bin', 'G (Track 2).bin']), ['G.cue']);
  assert.deepStrictEqual(scan.dropTrackFiles(['G.m3u', 'G (Disc 1).cue', 'G (Disc 1).bin', 'G (Disc 2).chd']), ['G.m3u']);
  assert.deepStrictEqual(scan.dropTrackFiles(['A.bin', 'B.iso']), ['A.bin', 'B.iso']);
});

test('scanFolder finds games by folder and extension, and skips other files', async () => {
  const { scan, root, touch } = setup();
  touch(
    'snes/Chrono Trigger (USA).sfc',
    'snes/readme.txt',
    'psx/Final Fantasy VII/FF7.cue',
    'psx/Final Fantasy VII/FF7 (Track 1).bin',
    'misc/Earthbound.smc',
    'misc/mystery.iso',
    'snes/.hidden/Secret.sfc',
    'snes/media/cover.png'
  );
  const found = await scan.scanFolder(root);
  const byName = Object.fromEntries(found.map((f) => [f.name, f]));
  assert.deepStrictEqual(names(found), ['Chrono Trigger', 'Earthbound', 'FF7', 'mystery']);
  assert.strictEqual(byName['Chrono Trigger'].platform, 'snes');
  assert.strictEqual(byName['Chrono Trigger'].folder, 'snes');
  assert.strictEqual(byName.FF7.platform, 'psx');
  assert.strictEqual(byName.Earthbound.platform, 'snes');
  assert.strictEqual(byName.mystery.platform, null);
  assert.strictEqual(byName['Chrono Trigger'].target, path.join(root, 'snes', 'Chrono Trigger (USA).sfc'));
});

test('scanFolder applies the user platform picks to a folder and its subfolders', async () => {
  const { scan, root, touch } = setup();
  touch('misc/a.iso', 'misc/deep/b.iso', 'other/c.iso');
  const found = await scan.scanFolder(root, { platforms: { MISC: 'ps2' } });
  const byName = Object.fromEntries(found.map((f) => [f.name, f.platform]));
  assert.deepStrictEqual(byName, { a: 'ps2', b: 'ps2', c: null });
});

test('scanFolder without subfolders stays at the top level', async () => {
  const { scan, root, touch } = setup();
  touch('Top.sfc', 'snes/Deep.sfc');
  assert.deepStrictEqual(names(await scan.scanFolder(root, { recursive: false })), ['Top']);
});

test('scanFolder for videos takes video files only and reads the year', async () => {
  const { scan, root, touch } = setup();
  touch('Movies/Arrival (2016).mkv', 'Movies/Arrival (2016).srt', 'Movies/poster.jpg', 'Shows/S01/Pilot.mp4');
  const found = await scan.scanFolder(root, { library: 'videos' });
  assert.deepStrictEqual(names(found), ['Arrival', 'Pilot']);
  assert.strictEqual(found.find((f) => f.name === 'Arrival').year, 2016);
  assert.ok(found.every((f) => f.platform === null));
});

test('scanFolder on a missing folder finds nothing instead of throwing', async () => {
  const { scan, root } = setup();
  assert.deepStrictEqual(await scan.scanFolder(path.join(root, 'nope')), []);
});

test('summarize groups by folder and platform', () => {
  const { scan } = setup();
  const found = [
    { folder: 'snes', platform: 'snes' },
    { folder: 'snes', platform: 'snes' },
    { folder: 'misc', platform: null },
  ];
  assert.deepStrictEqual(scan.summarize(found), [
    { folder: 'misc', platform: null, count: 1 },
    { folder: 'snes', platform: 'snes', count: 2 },
  ]);
});

test('mergeScan adds new files, keeps known ones and marks missing ones unavailable', () => {
  const { scan } = setup();
  const root = 'D:\\ROMs';
  const kept = { id: 'k', library: 'games', name: 'Kept', target: 'D:\\ROMs\\snes\\Kept.sfc', available: true, launchCount: 4 };
  const gone = { id: 'g', library: 'games', name: 'Gone', target: 'D:\\ROMs\\snes\\Gone.sfc', available: true };
  const back = { id: 'b', library: 'games', name: 'Back', target: 'D:\\ROMs\\snes\\Back.sfc', available: false };
  const elsewhere = { id: 'e', library: 'games', name: 'Else', target: 'E:\\Other\\Else.sfc', available: true };
  const sibling = { id: 's', library: 'games', name: 'Sib', target: 'D:\\ROMs2\\Sib.sfc', available: true };
  const video = { id: 'v', library: 'videos', name: 'Vid', target: 'D:\\ROMs\\Vid.mkv', available: true };
  const found = [
    { target: 'd:\\roms\\SNES\\kept.sfc', name: 'Kept', year: null, platform: 'snes' },
    { target: 'D:\\ROMs\\snes\\Back.sfc', name: 'Back', year: null, platform: 'snes' },
    { target: 'D:\\ROMs\\snes\\New.sfc', name: 'New', year: null, platform: 'snes' },
    { target: 'D:\\ROMs\\snes\\New.sfc', name: 'New', year: null, platform: 'snes' },
  ];

  const result = scan.mergeScan([kept, gone, back, elsewhere, sibling, video], found, { root, now: 42 });
  assert.deepStrictEqual([result.added, result.missing, result.restored], [1, 1, 1]);
  const byId = Object.fromEntries(result.entries.map((e) => [e.id, e]));
  assert.strictEqual(byId.k, kept, 'unchanged entries are the same object');
  assert.strictEqual(byId.g.available, false);
  assert.strictEqual(byId.b.available, true);
  assert.strictEqual(byId.e, elsewhere);
  assert.strictEqual(byId.s, sibling, 'D:\\ROMs2 is not inside D:\\ROMs');
  assert.strictEqual(byId.v, video, 'other libraries are untouched');

  const added = result.entries.find((e) => e.name === 'New');
  assert.match(added.id, /^[0-9a-f-]{36}$/);
  assert.deepStrictEqual(
    [added.library, added.platform, added.type, added.available, added.createdAt],
    ['games', 'snes', 'profile', true, 42]
  );
  assert.strictEqual(gone.available, true, 'the input list is not mutated');
});

test('mergeScan stores a video year in meta', () => {
  const { scan } = setup();
  const found = [{ target: 'D:\\Movies\\Arrival (2016).mkv', name: 'Arrival', year: 2016, platform: null }];
  const { entries } = scan.mergeScan([], found, { root: 'D:\\Movies', library: 'videos' });
  assert.deepStrictEqual(entries[0].meta, { year: 2016 });
  assert.strictEqual(entries[0].library, 'videos');
});
