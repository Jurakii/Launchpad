const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { EventEmitter } = require('events');
const { freshUserData, freshRequire } = require('./helpers');

const ID = '11111111-1111-4111-8111-111111111111';
const MPV = 'C:\\Program Files\\mpv\\mpv.exe';
const profiles = {
  profiles: [{ id: 'mpv', name: 'mpv', kind: 'player', exe: MPV, args: ['--fs', '{file}'], platforms: ['videos'] }],
  defaults: { videos: 'mpv' },
};
const video = (extra = {}) => ({
  id: ID,
  library: 'videos',
  name: 'Arrival',
  platform: null,
  type: 'profile',
  profile: null,
  target: 'D:\\Movies\\Arrival & Co (2016).mkv',
  available: true,
  ...extra,
});

// A spawn stand-in: records calls and hands back a child the test drives.
function fakeSpawn({ fail } = {}) {
  const calls = [];
  const children = [];
  const spawn = (exe, args, opts) => {
    calls.push({ exe, args, opts });
    const child = new EventEmitter();
    child.pid = 1000 + children.length;
    child.kill = () => child.emit('exit', null);
    children.push(child);
    setImmediate(() => (fail ? child.emit('error', new Error(fail)) : child.emit('spawn')));
    return child;
  };
  return { spawn, calls, children };
}

function setup(deps = {}) {
  freshUserData();
  const launch = freshRequire('../lib/media/launch');
  let clock = 0;
  const fake = fakeSpawn(deps.spawnOpts);
  const launcher = launch.createLauncher({
    spawn: fake.spawn,
    exists: () => true,
    now: () => clock,
    ...deps,
  });
  const exits = [];
  launcher.on('exit', (info) => exits.push(info));
  return { launch, launcher, fake, exits, tick: (ms) => (clock += ms) };
}

test('a profile launch spawns the player with the file as one argument and no shell', async () => {
  const { launcher, fake } = setup();
  const result = await launcher.launch(video(), profiles);
  assert.deepStrictEqual(result, { tracked: true, pid: 1000 });
  const [call] = fake.calls;
  assert.strictEqual(call.exe, MPV);
  assert.deepStrictEqual(call.args, ['--fs', 'D:\\Movies\\Arrival & Co (2016).mkv']);
  assert.strictEqual(call.opts.shell, false);
  assert.strictEqual(call.opts.cwd, 'C:\\Program Files\\mpv');
});

test('a tracked launch is listed while running and reports its exit with the time played', async () => {
  const { launcher, fake, exits, tick } = setup();
  await launcher.launch(video(), profiles);
  assert.deepStrictEqual(launcher.list().map((r) => r.id), [ID]);
  assert.ok(launcher.isRunning(ID));

  tick(65_000);
  fake.children[0].emit('exit', 0);
  assert.deepStrictEqual(exits, [{ id: ID, code: 0, seconds: 65, handoff: false, stopped: false }]);
  assert.deepStrictEqual(launcher.list(), []);
});

test('an exit right after starting is reported as a hand-off', async () => {
  const { launcher, fake, exits, tick, launch } = setup();
  await launcher.launch(video(), profiles);
  tick(launch.HANDOFF_MS - 1);
  fake.children[0].emit('exit', 0);
  assert.strictEqual(exits[0].handoff, true);
});

test('launching something already running does not start it twice', async () => {
  const { launcher, fake } = setup();
  await launcher.launch(video(), profiles);
  assert.deepStrictEqual(await launcher.launch(video(), profiles), { alreadyRunning: true });
  assert.strictEqual(fake.calls.length, 1);
});

test('stop ends a tracked launch, and its exit follows', async () => {
  const { launcher, exits } = setup();
  await launcher.launch(video(), profiles);
  assert.strictEqual(launcher.stop(ID), true);
  assert.strictEqual(exits.length, 1);
  assert.strictEqual(exits[0].stopped, true);
  assert.strictEqual(exits[0].handoff, false, 'closing it quickly is not a hand-off');
  assert.strictEqual(launcher.stop(ID), false);
});

test('a program that will not start rejects and is not listed', async () => {
  const { launcher } = setup({ spawnOpts: { fail: 'spawn ENOENT' } });
  await assert.rejects(launcher.launch(video(), profiles), /ENOENT/);
  assert.deepStrictEqual(launcher.list(), []);
});

test('missing files and unavailable entries are refused before spawning', async () => {
  const { launcher, fake } = setup({ exists: () => false });
  await assert.rejects(launcher.launch(video(), profiles), /Can't find D:\\Movies/);
  await assert.rejects(launcher.launch(video({ available: false }), profiles), /Can't find/);
  assert.strictEqual(fake.calls.length, 0);
});

test('no profile for the platform gives a message naming it', async () => {
  const { launcher } = setup();
  const game = video({ library: 'games', platform: 'psx' });
  await assert.rejects(launcher.launch(game, profiles), /No player or emulator is set up for psx/);
  await assert.rejects(launcher.launch(video(), { profiles: [], defaults: {} }), /set up for videos/);
});

test('other launch types go through the normal launcher and are not tracked', async () => {
  const launched = [];
  const { launcher, fake } = setup({ launchOther: async (e) => launched.push(e.target) });
  const steam = video({ type: 'url', target: 'steam://rungameid/1145360' });
  assert.deepStrictEqual(await launcher.launch(steam, profiles), { tracked: false });
  assert.deepStrictEqual(launched, ['steam://rungameid/1145360']);
  assert.strictEqual(fake.calls.length, 0);
});

test('recordLaunch and recordPlay update one entry and skip hand-offs', () => {
  const { launch } = setup();
  const entries = [video(), video({ id: 'other' })];
  const after = launch.recordLaunch(entries, ID, 50);
  assert.deepStrictEqual([after[0].launchCount, after[0].lastLaunched], [1, 50]);
  assert.strictEqual(after[1], entries[1]);
  assert.strictEqual(entries[0].launchCount, undefined, 'input not mutated');

  const played = launch.recordPlay(after, { id: ID, seconds: 30, handoff: false });
  assert.strictEqual(launch.recordPlay(played, { id: ID, seconds: 10, handoff: false })[0].playSeconds, 40);
  assert.strictEqual(launch.recordPlay(after, { id: ID, seconds: 1, handoff: true }), after);
});

test('a real process is spawned, tracked and reported when it exits', async () => {
  const dir = freshUserData();
  const launch = freshRequire('../lib/media/launch');
  const file = path.join(dir, 'clip & more.mkv');
  fs.writeFileSync(file, '');
  // node.exe stands in for a player: it checks it got the file as one argument.
  const node = {
    profiles: [
      {
        id: 'node',
        name: 'node',
        kind: 'player',
        exe: process.execPath,
        args: ['-e', 'process.exit(process.argv[1] === process.env.EXPECT ? 7 : 1)', '{file}'],
        platforms: ['videos'],
      },
    ],
    defaults: { videos: 'node' },
  };
  process.env.EXPECT = file;
  const launcher = launch.createLauncher();
  const exited = new Promise((resolve) => launcher.on('exit', resolve));
  const result = await launcher.launch(video({ target: file }), node);
  assert.strictEqual(result.tracked, true);
  const info = await exited;
  assert.strictEqual(info.code, 7);
  assert.strictEqual(info.handoff, true);
});
