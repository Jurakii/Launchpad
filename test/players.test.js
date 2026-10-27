const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');

const MPV = 'C:\\Program Files\\mpv\\mpv.exe';

function setup() {
  const dir = freshUserData();
  const players = freshRequire('../lib/media/players');
  return { dir, players, file: path.join(dir, 'profiles.json') };
}

const profile = (extra = {}) => ({
  id: 'mpv',
  name: 'mpv',
  kind: 'player',
  exe: MPV,
  args: ['--fs', '{file}'],
  platforms: ['videos'],
  ...extra,
});

test('buildLaunch passes the file as one argument, however it is named', () => {
  const { players } = setup();
  const names = [
    'C:\\Movies\\Blade Runner (2017).mkv',
    'C:\\Movies\\Say "Hi".mkv',
    'C:\\ROMs\\Tom & Jerry.sfc',
    'C:\\ROMs\\100% Orange.sfc',
    'C:\\ROMs\\Cash $& $1 $$.sfc',
  ];
  for (const file of names) {
    const { exe, args } = players.buildLaunch(profile(), file);
    assert.strictEqual(exe, MPV);
    assert.deepStrictEqual(args, ['--fs', file]);
  }
});

test('buildLaunch fills {dir} and {name} inside arguments and runs from the program folder', () => {
  const { players } = setup();
  const p = profile({ args: ['--title={name}', '--cd', '{dir}', '{file}'] });
  const out = players.buildLaunch(p, 'D:\\Shows\\S01\\Pilot.mkv');
  assert.deepStrictEqual(out.args, ['--title=Pilot', '--cd', 'D:\\Shows\\S01', 'D:\\Shows\\S01\\Pilot.mkv']);
  assert.strictEqual(out.cwd, 'C:\\Program Files\\mpv');
});

test('buildLaunch leaves unknown braces alone', () => {
  const { players } = setup();
  const out = players.buildLaunch(profile({ args: ['{core}', '{file}'] }), 'C:\\a.sfc');
  assert.deepStrictEqual(out.args, ['{core}', 'C:\\a.sfc']);
});

test('buildLaunch refuses an invalid profile or a missing file', () => {
  const { players } = setup();
  assert.throws(() => players.buildLaunch(profile({ exe: 'C:\\run.bat' }), 'C:\\a.mkv'), /Batch files/);
  assert.throws(() => players.buildLaunch(profile(), ''), /No file/);
});

test('validateProfile rejects batch files, relative paths and non-exe programs', () => {
  const { players } = setup();
  assert.deepStrictEqual(players.validateProfile(profile()), []);
  for (const exe of ['C:\\x\\run.bat', 'C:\\x\\RUN.CMD']) {
    assert.match(players.validateProfile(profile({ exe }))[0], /Batch files/);
  }
  assert.match(players.validateProfile(profile({ exe: 'mpv.exe' }))[0], /full path/);
  assert.match(players.validateProfile(profile({ exe: 'C:\\x\\play.ps1' }))[0], /\.exe/);
});

test('validateProfile checks id, name, kind, args and platforms', () => {
  const { players } = setup();
  assert.ok(players.validateProfile(profile({ id: '../x' })).length);
  assert.ok(players.validateProfile(profile({ name: ' ' })).length);
  assert.ok(players.validateProfile(profile({ kind: 'store' })).length);
  assert.ok(players.validateProfile(profile({ args: '--fs {file}' })).length);
  assert.ok(players.validateProfile(profile({ args: ['--fs', 3] })).length);
  assert.ok(players.validateProfile(profile({ platforms: ['Super Nintendo'] })).length);
  assert.ok(players.validateProfile(null).length);
});

test('every preset is valid once a program is chosen', () => {
  const { players } = setup();
  for (const preset of players.PRESETS) {
    const p = players.fromPreset(preset.id, 'C:\\Apps\\prog.exe');
    assert.deepStrictEqual(players.validateProfile(p), [], preset.id);
    assert.strictEqual(p.candidates, undefined);
  }
  assert.throws(() => players.fromPreset('nope', MPV), /Unknown preset/);
});

test('fromPreset copies, so editing a profile never changes the preset', () => {
  const { players } = setup();
  const p = players.fromPreset('mpv', MPV);
  p.args.push('--mute');
  assert.deepStrictEqual(players.PRESETS.find((x) => x.id === 'mpv').args, ['--fs', '{file}']);
});

test('detectInstalled returns presets whose program exists', () => {
  const { players } = setup();
  const found = players.detectInstalled((p) => p === 'C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe');
  assert.deepStrictEqual(found.map((p) => [p.id, p.exe]), [['vlc', 'C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe']]);
});

test('Dolphin and RetroArch (mGBA) presets sit beside the SNES one, on platform ids the scanner uses', () => {
  const { players } = setup();
  const scan = require('../lib/media/scan');
  const known = new Set(scan.PLATFORMS.map((p) => p.id));
  const byId = Object.fromEntries(players.PRESETS.map((p) => [p.id, p]));
  assert.ok(byId['retroarch-snes'], 'the SNES preset is kept');
  assert.deepStrictEqual(byId.dolphin.platforms, ['gamecube', 'wii']);
  assert.deepStrictEqual(byId.dolphin.args, ['-b', '-e', '{file}']);
  assert.deepStrictEqual(byId['retroarch-gba'].platforms, ['gba']);
  assert.ok(byId['retroarch-gba'].args.includes('cores\\mgba_libretro.dll'));
  for (const preset of players.PRESETS) {
    for (const id of preset.platforms) assert.ok(id === players.VIDEOS || known.has(id), `${preset.id}: ${id}`);
  }
});

test('an installed RetroArch is offered as both presets, and Dolphin is found in Program Files', () => {
  const { players } = setup();
  const found = players.detectInstalled((p) => p === 'C:\\RetroArch-Win64\\retroarch.exe' || p === 'C:\\Program Files\\Dolphin\\Dolphin.exe');
  assert.deepStrictEqual(found.map((p) => p.id).sort(), ['dolphin', 'retroarch-gba', 'retroarch-snes']);
  assert.ok(found.every((p) => p.candidates === undefined));
});

test('missing profiles.json loads as empty, and save round-trips', () => {
  const { players, file } = setup();
  assert.deepStrictEqual(players.load(), { profiles: [], defaults: {} });
  players.save({ profiles: [profile()], defaults: { videos: 'mpv' } });
  assert.deepStrictEqual(players.load(), { profiles: [profile()], defaults: { videos: 'mpv' } });
  assert.ok(fs.existsSync(file));
});

test('save drops invalid and duplicate profiles and defaults that point nowhere', () => {
  const { players } = setup();
  const saved = players.save({
    profiles: [profile(), profile({ name: 'dup' }), profile({ id: 'bad', exe: 'C:\\a.bat' })],
    defaults: { videos: 'mpv', snes: 'missing', 'Bad Key': 'mpv' },
  });
  assert.deepStrictEqual(saved.profiles.map((p) => p.name), ['mpv']);
  assert.deepStrictEqual(saved.defaults, { videos: 'mpv' });
});

test('an unreadable profiles.json is moved aside once and loads as empty', () => {
  const { dir, players, file } = setup();
  fs.writeFileSync(file, 'not json');
  assert.deepStrictEqual(players.load(), { profiles: [], defaults: {} });
  assert.ok(!fs.existsSync(file));
  assert.strictEqual(listDir(dir).filter((f) => f.startsWith('profiles.corrupt-')).length, 1);
  players.save({ profiles: [profile()], defaults: {} });
  assert.strictEqual(players.load().profiles.length, 1);
});

test('profileFor uses the entry profile, then the platform or videos default', () => {
  const { players } = setup();
  const ra = profile({ id: 'ra', kind: 'emulator', exe: 'C:\\RA\\retroarch.exe', platforms: ['snes'] });
  const data = { profiles: [profile(), ra], defaults: { videos: 'mpv', snes: 'ra' } };
  assert.strictEqual(players.profileFor({ library: 'games', platform: 'snes', profile: null }, data).id, 'ra');
  assert.strictEqual(players.profileFor({ library: 'videos', platform: null, profile: null }, data).id, 'mpv');
  assert.strictEqual(players.profileFor({ library: 'games', platform: 'snes', profile: 'mpv' }, data).id, 'mpv');
  assert.strictEqual(players.profileFor({ library: 'games', platform: 'psx', profile: null }, data), null);
  assert.strictEqual(players.profileFor({ library: 'games', platform: 'snes', profile: 'gone' }, data), null);
});
