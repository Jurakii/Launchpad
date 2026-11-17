// Writes the 1.2.2 compatibility fixtures in ./v1.2.2 with Launchpad
// 1.2.2's own lib/ modules (see js-env.js), for crates/core/tests/compat.rs.
//
//   node crates/core/tests/fixtures/generate.js     (from the repo root; needs npm install)
//
// Re-runnable: it deletes ./v1.2.2 and writes it again. Ids and times are
// fixed, so the output only changes when the JS does. What it writes:
//
//   data/          a data folder built through the modules' public functions:
//                  apps.json, settings.json, layouts.json, library.json,
//                  profiles.json, icons/, files/ (one stored file, one stored
//                  folder, their thumbnails). Each file is checked to come
//                  back byte for byte from a JS load + save.
//   backups/       v3.json (+ v3-files/) from main.js's export logic, and the
//                  older v2.json and bare.json (see below).
//   imported/<b>/  what the JS import of each backup left in an empty data
//                  folder, plus result.json: storedMissing, the errors, and
//                  every file it wrote.
//   hand-edited/   input/: files as a user or an older version might have
//                  left them (wrong types, unknown keys, the old 'folder'
//                  kind, number forms JSON.stringify never writes); and
//                  js-resaved/: those files after a 1.2.2 load + save.
//
// Absolute paths in the JSON (icon paths, stored targets) are written as
// {{DATA}} where the data folder was, so the tests can put any folder there.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { useUserData, exportBackup, importBackup, listFiles } = require('./js-env');

const OUT = path.join(__dirname, 'v1.2.2');
const T0 = 1767323045000; // 2026-01-02T03:04:05Z
const id = (n) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`;
// A 1x1 PNG, and a second one so icons and thumbnails can't be mixed up unseen.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');
const THUMB = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR42mP4z8AAAAMBAQAY3Y2wAAAAAElFTkSuQmCC', 'base64');

function write(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, data);
}

function assertSame(a, b, what) {
  if (!a.equals(b)) throw new Error(`${what}: a 1.2.2 load + save changed the file`);
}

// Copies `from` to `to`, writing `dir` inside .json files as {{DATA}}.
function copyOut(from, to, dir) {
  const escaped = JSON.stringify(dir).slice(1, -1);
  for (const rel of listFiles(from)) {
    const src = path.join(from, rel);
    let data = fs.readFileSync(src);
    if (rel.endsWith('.json')) data = Buffer.from(data.toString('utf-8').split(escaped).join('{{DATA}}'), 'utf-8');
    write(path.join(to, rel), data);
  }
}

function buildData(m, dir) {
  const { store, settings, layouts, controller, library, players, defaults } = m;
  const icon = (n) => {
    const file = path.join(store.iconsDir(), `${id(n)}.png`);
    write(file, PNG);
    return file;
  };
  let next = 1;
  const [explorer, recycleBin] = defaults.buildEntries(() => id(next++), T0);
  explorer.iconPath = icon(1);
  const stored = (n, name, kind) => {
    const folder = path.join(store.filesDir(), id(n));
    if (kind === 'file') write(path.join(folder, name), 'hello');
    else write(path.join(folder, name, 'inside.txt'), 'inside');
    write(path.join(store.filesDir(), 'thumbs', `${id(n)}.png`), THUMB);
  };
  stored(15, 'Notes.txt', 'file');
  stored(16, 'Photos été', 'folder');
  store.save([
    explorer,
    recycleBin,
    { id: id(10), kind: 'island', name: 'Tools 日本語', parentId: null, collapsed: false, w: 4, h: 3, showNames: true, color: '#3366ff', createdAt: T0 + 10, gridX: 2, gridY: 0 },
    // Two deep: an island inside that island, and an app inside it.
    { id: id(11), kind: 'island', name: 'Café ☕', parentId: id(10), createdAt: T0 + 11 },
    {
      id: id(12),
      kind: 'app',
      name: 'Terminal 🖥️',
      type: 'path',
      target: 'C:\\Windows\\System32\\cmd.exe',
      args: '/k echo 日本語',
      cwd: 'C:\\',
      parentId: id(11),
      pinned: true, // no pinOrder: load() gives it one
      createdAt: T0 + 12,
      lastLaunched: T0 + 500,
      launchCount: 3,
      iconPath: icon(12),
      futureKey: { nested: [1, 2.5, 'x', null], flag: false },
    },
    { id: id(13), kind: 'app', name: 'Build script', type: 'script', target: 'C:\\scripts\\build.ps1', interpreter: 'powershell', parentId: id(10), createdAt: T0 + 13, gridX: 0, gridY: 0 },
    { id: id(14), kind: 'app', name: 'Example — Docs', type: 'url', target: 'https://example.com/docs?q=1&x=ü', pinned: true, pinOrder: 0, color: '#ff8800', iconPath: icon(14), createdAt: T0 + 14, zoom: 1.25 },
    { id: id(15), kind: 'app', name: 'Notes.txt', type: 'stored', storedName: 'Notes.txt', storedKind: 'file', originalPath: 'C:\\Users\\Me\\Desktop\\Notes.txt', size: 5, thumbState: 'ready', createdAt: T0 + 15 },
    { id: id(16), kind: 'app', name: 'Photos été', type: 'stored', storedName: 'Photos été', storedKind: 'folder', originalPath: 'D:\\Photos été', size: 6, thumbState: 'generic', createdAt: T0 + 16 },
  ]);
  store.load(); // fills in positions and pin orders, and saves

  settings.save({
    hotkey: 'Alt+Space',
    startFullscreen: false,
    minimizeToTrayOnClose: true,
    notifyUpdates: true,
    disableHotkeyInApps: false,
    googleFaviconFallback: false,
    organizerEnabled: true,
    organizerPanel: 'autoHide',
    organizerSnapOnDrag: true,
    organizerHotkey: 'CommandOrControl+Alt+H',
    fileStoreOnDrop: 'copy',
    fileStoreLimit: 2 * 1024 ** 3,
    libraries: [
      { root: 'D:\\Games', library: 'games', recursive: true, platforms: ['snes', 'gba'] },
      { root: 'E:\\Vidéos', library: 'videos', recursive: false, platforms: [] },
    ],
    futureSetting: { a: 1, list: ['x'] },
  });
  controller.save({ deadZone: 0.42, pads: { 'Xbox 360 Controller (XInput STANDARD GAMEPAD)': { 0: 'select', 1: 'back', 3: 'quick', 9: 'menu' } } });

  layouts.saveLayout({
    id: id(30),
    name: 'Wide 日本',
    cols: 3,
    rows: 2,
    zones: [
      { id: 'a', name: 'Left', x: 0, y: 0, w: 2, h: 2 },
      { id: 'b', name: 'Right', x: 2, y: 0, w: 1, h: 2 },
    ],
  });
  layouts.setHidden('thirds', true);
  layouts.setGap(12);
  const display = (n, x, width, height) => ({ id: n, bounds: { x, y: 0, width, height } });
  layouts.saveScene({
    id: id(31),
    name: 'Morning',
    islandId: null,
    savedAt: T0,
    windows: [
      { exe: 'C:\\Windows\\explorer.exe', title: 'Downloads', display: display(1, 0, 2560, 1440), rect: { x: 0, y: 0, w: 0.5, h: 1 }, maximized: false },
      { exe: 'C:\\Program Files\\App\\app.exe', title: 'Ünïcode — 🎵', display: display(2, -1920, 1920, 1080), rect: { x: 0.3333, y: 0.25, w: 0.6667, h: 0.75 }, maximized: true },
    ],
  });
  layouts.saveScene({
    id: id(32),
    name: 'Tools scene',
    islandId: id(10),
    savedAt: T0 + 1,
    windows: [{ exe: 'C:\\scripts\\tool.exe', title: '', display: display(1, 0, 2560, 1440), rect: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 }, maximized: false }],
  });
  layouts.save({ ...layouts.load(), futureLayoutsKey: [1, 'two'] });

  library.save([
    library.normalize({
      id: id(40),
      library: 'games',
      name: 'Chrono Trigger',
      platform: 'snes',
      type: 'profile',
      profile: null,
      target: 'D:\\Games\\SNES\\Chrono Trigger (USA).sfc',
      launchCount: 2,
      lastLaunched: T0 + 5,
      playSeconds: 3600,
      createdAt: T0,
      media: { cover: { path: 'cover.jpg', source: 'user', updatedAt: T0 }, logo: { path: 'logo.png', source: 'scraper:steamgriddb', updatedAt: T0 + 1 } },
      futureEntryKey: true,
    }),
    library.normalize({ id: id(41), library: 'videos', name: 'Amélie', type: 'path', target: 'E:\\Vidéos\\Amélie (2001).mkv', meta: { year: 2001 }, available: false, createdAt: T0 + 1 }),
    library.normalize({ id: id(42), library: 'games', name: 'Steam game 🎮', type: 'url', target: 'steam://rungameid/123', createdAt: T0 + 2 }),
  ]);
  players.save({
    profiles: [
      players.fromPreset('mpv', 'C:\\Program Files\\mpv\\mpv.exe'),
      players.fromPreset('retroarch-snes', 'C:\\RetroArch-Win64\\retroarch.exe'),
      { id: 'custom-emu', name: 'My Emu', kind: 'emulator', exe: 'C:\\Emus\\emu.exe', args: ['--rom', '{file}', '--dir', '{dir}'], platforms: ['gba'] },
    ],
    defaults: { videos: 'mpv', snes: 'retroarch-snes', gba: 'custom-emu' },
  });

  // The fixtures are what 1.2.2 writes, so loading and saving them again in
  // 1.2.2 must not change a byte.
  const files = ['apps.json', 'settings.json', 'layouts.json', 'library.json', 'profiles.json'];
  const before = files.map((f) => fs.readFileSync(path.join(dir, f)));
  resave(m);
  files.forEach((f, i) => assertSame(before[i], fs.readFileSync(path.join(dir, f)), f));
  fs.rmSync(path.join(dir, 'apps.json.bak'), { force: true });
  fs.rmSync(path.join(dir, 'library.json.bak'), { force: true });
}

// A 1.2.2 load followed by a save with no change, for each file.
function resave({ store, settings, layouts, library, players }) {
  store.save(store.load());
  settings.save({});
  layouts.save(layouts.load());
  library.save(library.load());
  players.save(players.load());
}

// Written as text, not through JSON.stringify, to keep forms like 2.0 and 1e3.
const HAND_EDITED = {
  'apps.json': `[
  { "id": "${id(50)}", "kind": "folder", "name": "Old folder", "gridX": 0, "gridY": 0, "createdAt": 1.0e12 },
  { "id": "${id(51)}", "name": "Very old entry", "type": "path", "target": "C:\\\\old.exe", "parentId": "${id(50)}" },
  { "id": "${id(52)}", "kind": "app", "name": "Wrong types", "pinned": "yes", "launchCount": "7", "gridX": "3", "gridY": 2.0, "color": 5, "collapsed": null, "createdAt": null },
  { "id": "${id(53)}", "kind": "app", "name": "Stray parent", "parentId": "nowhere", "gridX": 0, "gridY": 0, "extra": { "deep": [1.5, -0, 1e3, 12345678901234567890, 0.1] } },
  { "id": "${id(54)}", "kind": "app", "type": "stored", "name": "Escaped", "storedName": "..\\\\evil.txt", "storedKind": "weird", "thumbState": 3, "target": "C:\\\\Windows\\\\notepad.exe", "iconPath": "C:\\\\x.png" },
  { "id": "not-a-uuid", "kind": "app", "type": "stored", "name": "Bad id", "storedName": "a.txt" },
  { "id": "${id(55)}", "kind": "island", "name": "Island \\u00e9\\/\\u0001", "w": 2, "pinned": true, "pinOrder": 1 },
  { "id": "${id(56)}", "kind": "app", "name": "Pinned", "pinned": true },
  null,
  "a string",
  [1, 2],
  { "id": "${id(57)}", "kind": "app", "name": "Same cell", "gridX": 0, "gridY": 0, "createdAt": 5 }
]
`,
  'settings.json': `{
  "hotkey": 5,
  "organizerPanel": "sideways",
  "controller": "not an object",
  "libraries": { "root": "D:\\\\Games" },
  "fileStoreLimit": 1e3,
  "float": 2.0,
  "negativeZero": -0,
  "big": 12345678901234567890,
  "nested": { "b": 1, "a": [true, null, { "x": "\\u2028" }] }
}
`,
  'layouts.json': `{
  "version": 0,
  "futureKey": { "x": 1 },
  "gap": "8",
  "hidden": ["halves", "halves", "nope", "${id(60)}"],
  "layouts": [
    { "id": "${id(60)}", "name": "  Spaced name  ", "cols": 2.0, "rows": 1, "zones": [{ "id": "z", "name": "Z", "x": 0, "y": 0, "w": 2, "h": 1, "zoneExtra": "kept?" }], "layoutExtra": 1 },
    { "id": "halves", "name": "Not a built-in", "cols": 1, "rows": 1, "zones": [{ "id": "z", "name": "Z", "x": 0, "y": 0, "w": 1, "h": 1 }] },
    { "id": "broken", "name": "Overlap", "cols": 1, "rows": 1, "zones": [{ "id": "a", "x": 0, "y": 0, "w": 1, "h": 1 }, { "id": "b", "x": 0, "y": 0, "w": 1, "h": 1 }] }
  ],
  "scenes": [
    { "sceneExtra": "first", "id": "${id(61)}", "name": "Scene", "islandId": null, "savedAt": 1.5, "windows": [
      { "exe": "C:\\\\a.exe", "title": "t", "display": { "id": 1, "bounds": { "x": 0, "y": 0, "width": 100, "height": 100, "boundsExtra": 1 }, "displayExtra": 2 }, "rect": { "x": 0, "y": 0, "w": 1, "h": 1, "rectExtra": 3 }, "maximized": false, "windowExtra": 4 }
    ] },
    { "id": "${id(62)}", "name": "No island key", "windows": [{ "exe": "C:\\\\b.exe", "title": "", "display": { "id": 1, "bounds": { "x": 0, "y": 0, "width": 1, "height": 1 } }, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 }, "maximized": true }] },
    { "id": "${id(63)}", "name": "", "windows": [] }
  ]
}
`,
  'library.json': `[
  { "id": "${id(70)}", "name": 5, "library": "music", "type": "nope", "media": { "cover": { "path": "c.jpg", "extra": 1 }, "banner": "bad", "unknownSlot": { "path": "x" } }, "meta": [1], "launchCount": "2", "createdAt": 1.0, "extra": { "k": [] } },
  { "id": "${id(71)}", "library": "videos", "media": null, "meta": { "year": 1999.0 } },
  null,
  [1]
]
`,
  'profiles.json': `{
  "profiles": [
    { "id": "good", "name": "Good", "kind": "player", "exe": "C:\\\\p\\\\good.exe", "args": ["{file}"], "platforms": ["videos"], "extra": "dropped by both" },
    { "id": "good", "name": "Duplicate", "kind": "player", "exe": "C:\\\\p\\\\dup.exe", "args": [], "platforms": [] },
    { "id": "bat", "name": "Batch", "kind": "player", "exe": "C:\\\\p\\\\run.bat", "args": [], "platforms": [] }
  ],
  "defaults": { "videos": "good", "snes": "missing", "BAD KEY": "good" },
  "topExtra": true
}
`,
};

async function main() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'launchpad-fixtures-'));
  try {
    fs.rmSync(OUT, { recursive: true, force: true });

    const dataDir = path.join(tmp, 'data');
    const m = useUserData(dataDir);
    buildData(m, dataDir);
    copyOut(dataDir, path.join(OUT, 'data'), dataDir);

    // v3 is main.js's export. 1.2.2 can no longer write the older forms, so
    // they're made from it as those versions laid them out: v2 is v3 without
    // Media's library and profiles; the bare array is the entries alone, from
    // before layouts were included. Both predate the file store, so they have
    // no stored entries.
    const backups = path.join(tmp, 'backups');
    fs.mkdirSync(backups);
    const v3File = path.join(backups, 'v3.json');
    await exportBackup(useUserData(dataDir), v3File);
    const v3 = JSON.parse(fs.readFileSync(v3File, 'utf-8'));
    const oldApps = v3.apps.filter((a) => a.type !== 'stored');
    fs.writeFileSync(path.join(backups, 'v2.json'), JSON.stringify({ version: 2, apps: oldApps, layouts: v3.layouts }, null, 2), 'utf-8');
    fs.writeFileSync(path.join(backups, 'bare.json'), JSON.stringify(oldApps, null, 2), 'utf-8');
    copyOut(backups, path.join(OUT, 'backups'), dataDir);

    for (const name of ['v3', 'v2', 'bare']) {
      const dir = path.join(tmp, `import-${name}`);
      const mi = useUserData(dir);
      mi.store.load(); // as the app does at startup: 1.2.2's import expects icons\ to exist
      const res = await importBackup(mi, path.join(backups, `${name}.json`));
      const result = { storedMissing: res.storedMissing, layoutsError: res.layoutsError, mediaError: res.mediaError, files: listFiles(dir) };
      fs.writeFileSync(path.join(dir, 'result.json'), JSON.stringify(result, null, 2));
      const keep = path.join(tmp, `kept-${name}`);
      for (const f of ['apps.json', 'layouts.json', 'library.json', 'profiles.json', 'result.json']) {
        if (fs.existsSync(path.join(dir, f))) write(path.join(keep, f), fs.readFileSync(path.join(dir, f)));
      }
      copyOut(keep, path.join(OUT, 'imported', name), dir);
    }

    const hand = path.join(tmp, 'hand');
    for (const [f, text] of Object.entries(HAND_EDITED)) write(path.join(OUT, 'hand-edited', 'input', f), text);
    for (const [f, text] of Object.entries(HAND_EDITED)) write(path.join(hand, f), text);
    resave(useUserData(hand));
    const resaved = path.join(tmp, 'hand-resaved');
    for (const f of Object.keys(HAND_EDITED)) write(path.join(resaved, f), fs.readFileSync(path.join(hand, f)));
    copyOut(resaved, path.join(OUT, 'hand-edited', 'js-resaved'), hand);
    console.log(`Wrote ${path.relative(process.cwd(), OUT)}`);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
