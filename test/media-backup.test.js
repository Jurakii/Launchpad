const test = require('node:test');
const assert = require('node:assert');
const { freshUserData, freshRequire } = require('./helpers');

const ID = '11111111-1111-4111-8111-111111111111';
const profile = { id: 'mpv', name: 'mpv', kind: 'player', exe: 'C:\\mpv\\mpv.exe', args: ['{file}'], platforms: ['videos'] };

function setup() {
  freshUserData();
  const library = freshRequire('../lib/media/library');
  const players = freshRequire('../lib/media/players');
  const backup = freshRequire('../lib/media/backup');
  return { library, players, backup };
}

test('export carries the library and profiles, and import puts them back', () => {
  const a = setup();
  a.library.save([a.library.normalize({ id: ID, library: 'videos', name: 'Arrival', target: 'D:\\Arrival.mkv', media: { cover: { path: 'cover.jpg', source: 'user' } } })]);
  a.players.save({ profiles: [profile], defaults: { videos: 'mpv' } });
  const data = JSON.parse(JSON.stringify(a.backup.exportData()));

  const b = setup();
  assert.strictEqual(b.backup.importData({ version: 3, ...data }), 1);
  const [entry] = b.library.load();
  assert.strictEqual(entry.id, ID, 'ids survive, so art folders still match');
  assert.deepStrictEqual(entry.media.cover, { path: 'cover.jpg', source: 'user' });
  assert.deepStrictEqual(b.players.load(), { profiles: [profile], defaults: { videos: 'mpv' } });
});

test('older backups leave the library and profiles alone', () => {
  const { library, players, backup } = setup();
  library.save([library.normalize({ id: ID, name: 'Keep', target: 'D:\\k.sfc' })]);
  players.save({ profiles: [profile], defaults: {} });
  assert.strictEqual(backup.importData({ version: 2, apps: [], layouts: {} }), null);
  assert.strictEqual(backup.importData([{ name: 'v1 entry' }]), null);
  assert.strictEqual(library.load()[0].name, 'Keep');
  assert.strictEqual(players.load().profiles.length, 1);
});

test('import cleans what it is given', () => {
  const { library, backup } = setup();
  backup.importData({ library: [null, { id: '../evil', name: 'X', library: 'music' }], profiles: { profiles: [{ id: 'bad' }] } });
  const [entry] = library.load();
  assert.ok(library.isValidId(entry.id));
  assert.strictEqual(entry.library, 'games');
});
