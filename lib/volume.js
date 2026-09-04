const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'volume.ps1');

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

async function getVolume() {
  const out = await runScript(['-Action', 'get']);
  return JSON.parse(out);
}

async function setVolume(pct) {
  await runScript(['-Action', 'set', '-Value', String(Math.round(pct))]);
}

async function setMuted(muted) {
  await runScript(['-Action', 'mute', '-Value', muted ? 'true' : 'false']);
}

module.exports = { getVolume, setVolume, setMuted };
