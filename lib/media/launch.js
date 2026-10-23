// Starts library entries for Media Mode and keeps track of the ones it can.
// Profile launches (a player or emulator from profiles.json) are spawned
// here without a shell and kept, so Media Mode knows when the game or film
// ends. The other launch types go through lib/launcher.js like grid items
// and are not tracked.
const { EventEmitter } = require('events');
const childProcess = require('child_process');
const fs = require('fs');
const players = require('./players');

// A tracked process that ends this soon after starting most likely handed
// off to another process (Steam, a launcher stub, a single-instance player
// that passed the file to its open window), so its exit is not the end of
// the game. The renderer shows "launched" instead of "running" for these.
const HANDOFF_MS = 3000;

// deps are injectable for tests: spawn, launchOther (lib/launcher.launch),
// exists, now.
function createLauncher(deps = {}) {
  const spawn = deps.spawn || childProcess.spawn;
  const launchOther = deps.launchOther || ((entry) => require('../launcher').launch(entry));
  const exists = deps.exists || fs.existsSync;
  const now = deps.now || Date.now;

  const events = new EventEmitter();
  const running = new Map(); // entry id -> { child, pid, startedAt, name }

  function spawnTracked(entry, { exe, args, cwd }) {
    return new Promise((resolve, reject) => {
      let child;
      try {
        child = spawn(exe, args, { cwd, shell: false, windowsHide: false, stdio: 'ignore' });
      } catch (err) {
        reject(err);
        return;
      }
      // 'spawn' or 'error' always comes first: a missing program surfaces
      // as an error here rather than as an exit.
      child.once('error', (err) => {
        if (!running.has(entry.id)) reject(err);
      });
      child.once('spawn', () => {
        const startedAt = now();
        running.set(entry.id, { child, pid: child.pid, startedAt, name: entry.name });
        child.once('exit', (code) => {
          const run = running.get(entry.id);
          const stopped = !!(run && run.stopped);
          running.delete(entry.id);
          const ms = now() - startedAt;
          events.emit('exit', {
            id: entry.id,
            code,
            seconds: Math.round(ms / 1000),
            // Closed from Media Mode: a quick exit is not a hand-off then.
            handoff: !stopped && ms < HANDOFF_MS,
            stopped,
          });
        });
        resolve({ tracked: true, pid: child.pid });
      });
    });
  }

  // Starts one entry. `profiles` is players.load()'s data. Resolves to
  // { tracked, pid? } or { alreadyRunning: true }; rejects with a message
  // meant for the user.
  async function launch(entry, profiles) {
    if (running.has(entry.id)) return { alreadyRunning: true };
    if (!entry.available || (entry.type !== 'url' && entry.type !== 'command' && !exists(entry.target))) {
      throw new Error(`Can't find ${entry.target || entry.name}. Rescan the library or check the drive is connected.`);
    }
    if (entry.type !== 'profile') {
      await launchOther(entry);
      return { tracked: false };
    }
    const profile = players.profileFor(entry, profiles);
    if (!profile) {
      const what = entry.library === 'videos' ? 'videos' : entry.platform || 'this game';
      throw new Error(`No player or emulator is set up for ${what}. Add one in Players and emulators.`);
    }
    return spawnTracked(entry, players.buildLaunch(profile, entry.target));
  }

  // Ends a tracked launch (the Game running screen's Close game). Its exit
  // event follows as usual.
  function stop(id) {
    const run = running.get(id);
    if (!run) return false;
    run.stopped = true;
    run.child.kill();
    return true;
  }

  function list() {
    return [...running].map(([id, r]) => ({ id, pid: r.pid, name: r.name, startedAt: r.startedAt }));
  }

  return { launch, stop, list, isRunning: (id) => running.has(id), on: events.on.bind(events) };
}

// Bumps an entry's launch count and last-launched time. Returns a new list;
// unknown ids leave it unchanged.
function recordLaunch(entries, id, at = Date.now()) {
  return entries.map((e) =>
    e.id === id ? { ...e, launchCount: (e.launchCount || 0) + 1, lastLaunched: at } : e
  );
}

// Adds the length of a tracked session to an entry's play time. Hand-offs
// (see HANDOFF_MS) don't count: their length says nothing about play time.
function recordPlay(entries, { id, seconds, handoff }) {
  if (handoff || !(seconds > 0)) return entries;
  return entries.map((e) => (e.id === id ? { ...e, playSeconds: (e.playSeconds || 0) + seconds } : e));
}

module.exports = { createLauncher, recordLaunch, recordPlay, HANDOFF_MS };
