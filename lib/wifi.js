const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'wifi.ps1');

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

async function listNetworks() {
  const out = await runScript(['-Action', 'list']);
  return JSON.parse(out);
}

async function connect(ssid) {
  const out = await runScript(['-Action', 'connect', '-Ssid', ssid]);
  return JSON.parse(out);
}

async function disconnect() {
  const out = await runScript(['-Action', 'disconnect']);
  return JSON.parse(out);
}

module.exports = { listNetworks, connect, disconnect };
