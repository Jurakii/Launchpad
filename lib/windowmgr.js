const { spawn } = require('child_process');
const readline = require('readline');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'windowmgr.ps1');

// Generous: the first request also waits for PowerShell to start and compile
// the helper (~300 ms), and a window that's busy can be slow to move.
const REQUEST_TIMEOUT_MS = 5000;

let child = null;
let nextId = 1;
const pending = new Map(); // id -> { resolve, reject, timer }

function failPending(err) {
  for (const { reject, timer } of pending.values()) {
    clearTimeout(timer);
    reject(err);
  }
  pending.clear();
}

// Started on first use rather than at launch: most sessions never snap a
// window. Unlike foreground.js there's no automatic restart - if the helper
// dies, the next request simply starts a new one.
function ensureStarted() {
  if (child) return child;
  const proc = spawn(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', scriptPath],
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
      // fails anything else still waiting and lets the next request start
      // a fresh one.
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
const monitorOf = (hwnd) => request({ cmd: 'monitor', hwnd });
const place = (hwnd, rect) => request({ cmd: 'place', hwnd, x: rect.x, y: rect.y, w: rect.width, h: rect.height });
const focus = (hwnd) => request({ cmd: 'focus', hwnd });

function stop() {
  if (!child) return;
  const proc = child;
  child = null;
  failPending(new Error('The window helper was stopped.'));
  proc.stdin.end(); // the helper exits when its stdin closes
}

module.exports = { monitorOf, place, focus, stop };
