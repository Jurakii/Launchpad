const { spawn } = require('child_process');
const readline = require('readline');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'windowmgr.ps1');

// Generous: the first request also waits for PowerShell to start and compile
// the helper (~300 ms), and a window that's busy can be slow to move.
const REQUEST_TIMEOUT_MS = 5000;
const RESTART_DELAY_MS = 2000;

let child = null;
let nextId = 1;
// Who needs the helper running even when nothing is asking it anything:
// 'organizer' (the panel and drag snapping) and 'hotkey' (suspending the
// hotkey while another app is focused).
const keepAliveOwners = new Set();
const pending = new Map(); // id -> { resolve, reject, timer }
const listeners = new Set();
let lastForeground = null; // the latest foreground event, while the helper runs

function failPending(err) {
  for (const { reject, timer } of pending.values()) {
    clearTimeout(timer);
    reject(err);
  }
  pending.clear();
}

function emit(event) {
  if (event.ev === 'foreground') lastForeground = event;
  else if (event.ev === 'helperExit') lastForeground = null;
  for (const fn of listeners) fn(event);
}

const keepAlive = () => keepAliveOwners.size > 0;

// Started on first request, or straight away while something needs its
// events (see keepAliveOwners). If it dies while still needed, it's
// restarted after a short delay; otherwise the next request simply starts a
// new one.
function ensureStarted() {
  if (child) return child;
  const proc = spawn(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', scriptPath, '-ExcludePid', String(process.pid)],
    { windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] }
  );
  child = proc;
  proc.stdout.setEncoding('utf8');
  readline.createInterface({ input: proc.stdout }).on('line', (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.ev) {
      emit(msg);
      return;
    }
    const req = pending.get(msg.id);
    if (!req) return;
    pending.delete(msg.id);
    clearTimeout(req.timer);
    req.resolve(msg);
  });
  const onGone = () => {
    if (child !== proc) return;
    child = null;
    failPending(new Error('The window helper stopped unexpectedly.'));
    emit({ ev: 'helperExit' });
    if (keepAlive()) setTimeout(() => keepAlive() && ensureStarted(), RESTART_DELAY_MS);
  };
  proc.on('exit', onGone);
  proc.on('error', onGone);
  proc.stdin.on('error', () => {}); // EPIPE when it died mid-write; 'exit' reports that
  return proc;
}

function request(cmd) {
  return new Promise((resolve, reject) => {
    const proc = ensureStarted();
    const id = nextId++;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error('The window helper did not respond.'));
      // A hung helper would time out every later request too. Killing it
      // fails anything else still waiting and lets a fresh one start.
      try {
        proc.kill();
      } catch {
        // ignore
      }
    }, REQUEST_TIMEOUT_MS);
    pending.set(id, { resolve, reject, timer });
    proc.stdin.write(JSON.stringify({ id, ...cmd }) + '\n');
  });
}

// Each resolves to the helper's reply: { ok: true, ... } or { ok: false, code }.
// Rects are physical pixels.
// { bounds, work, frame, minimized }: the window's monitor, and where it is on it.
const monitorOf = (hwnd) => request({ cmd: 'monitor', hwnd });
// max: maximize it once it's there (on that rect's monitor).
const place = (hwnd, rect, max = false) =>
  request({ cmd: 'place', hwnd, x: rect.x, y: rect.y, w: rect.width, h: rect.height, max });
// { windows: [{ hwnd, exe, title, frame: [l, t, r, b], minimized, maximized }] }:
// the open windows worth placing, top of the z-order first. Scenes and the
// window picker use this. It's stricter than the taskbar's list
// (lib/windows.js): it also leaves out windows that can't be resized,
// windows run as administrator (Windows won't let us move them), and the
// shell's own windows, since none of those can be placed.
const listWindows = () => request({ cmd: 'windows' });
const focus = (hwnd) => request({ cmd: 'focus', hwnd });
// Docks a window along the top of rect as an AppBar; replies { rect } as
// Explorer adjusted it. renew re-registers it after Explorer restarted.
const appbarSet = (hwnd, msg, rect, renew = false) =>
  request({ cmd: 'appbarSet', hwnd, msg, x: rect.x, y: rect.y, w: rect.width, h: rect.height, renew });
// ifGone: only if that window no longer exists (one a killed run left docked).
const appbarRemove = (hwnd, ifGone = false) => request({ cmd: 'appbarRemove', hwnd, ifGone });
// { monitors: [{ monitor: [l, t, r, b], window }, ...] }: for each monitor,
// the window that decides whether a fullscreen app has it ({ hwnd, covers,
// zoomed, own, foreground }, or null); lib/fullscreen.js decides. Windows in
// own (handles) are flagged own. debug: also reply with detail, one line per
// window that decided a monitor (for the panel's log).
const fullscreenMonitors = (own = [], debug = false) => request({ cmd: 'fullscreen', own, debug });
// { was }: makes the window topmost again if something took that away.
const ensureTopmost = (hwnd) => request({ cmd: 'topmost', hwnd });

// Events: { ev: 'ready', taskbarCreated } once started,
// { ev: 'foreground', hwnd, kind, visible, title, exe, snappable } whenever the
// foreground window or its title changes, { ev: 'dragStart', hwnd } (a
// snappable window is being moved), { ev: 'moveEnd', hwnd, cancelled }, plus
// { ev: 'helperExit' } when the helper goes away.
function onEvent(fn) {
  listeners.add(fn);
}

// The last foreground event, or null if the helper isn't running.
const currentForeground = () => lastForeground;

function setKeepAlive(owner, on) {
  if (on) {
    keepAliveOwners.add(owner);
    ensureStarted();
  } else {
    keepAliveOwners.delete(owner);
    if (!keepAlive()) stop();
  }
}

// Stops the helper even if an owner still wants it, as on quit. Closing
// stdin rather than killing it lets the helper give back its AppBars' space
// on the way out.
function stop() {
  keepAliveOwners.clear();
  if (!child) return;
  const proc = child;
  child = null;
  failPending(new Error('The window helper was stopped.'));
  emit({ ev: 'helperExit' });
  proc.stdin.end(); // the helper exits when its stdin closes
}

module.exports = {
  monitorOf,
  place,
  listWindows,
  focus,
  appbarSet,
  appbarRemove,
  fullscreenMonitors,
  ensureTopmost,
  onEvent,
  currentForeground,
  setKeepAlive,
  stop,
};
