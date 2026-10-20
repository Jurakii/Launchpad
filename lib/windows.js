const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'windows.ps1');

function runScript(args) {
  return new Promise((resolve, reject) => {
    execFile(
      'powershell',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', scriptPath, ...args],
      { windowsHide: true },
      (error, stdout) => {
        if (error) return reject(error);
        resolve(stdout.trim());
      }
    );
  });
}

// Every window the Windows taskbar would show, for Launchpad's own taskbar.
// The organizer lists windows through lib/windowmgr.js instead, which also
// leaves out the ones it couldn't move (fixed-size, run as administrator).
async function listWindows(excludePid) {
  const out = await runScript(['-Action', 'list', '-ExcludePid', String(excludePid)]);
  return JSON.parse(out);
}

async function focusWindow(handle) {
  const out = await runScript(['-Action', 'focus', '-Handle', String(handle)]);
  return JSON.parse(out);
}

module.exports = { listWindows, focusWindow };
