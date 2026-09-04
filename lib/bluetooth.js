const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'bluetooth.ps1');

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

async function listDevices() {
  const out = await runScript(['-Action', 'list']);
  return JSON.parse(out);
}

async function disconnectDevice(instanceId) {
  const out = await runScript(['-Action', 'disconnect', '-InstanceId', instanceId]);
  return JSON.parse(out);
}

module.exports = { listDevices, disconnectDevice };
