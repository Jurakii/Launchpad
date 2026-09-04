const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'audio-devices.ps1');

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
  const out = await runScript(['-Action', 'listDevices']);
  return JSON.parse(out);
}

async function setDevice(deviceId) {
  const out = await runScript(['-Action', 'setDevice', '-DeviceId', deviceId]);
  return JSON.parse(out);
}

async function listSessions() {
  const out = await runScript(['-Action', 'listSessions']);
  return JSON.parse(out);
}

async function setSessionVolume(pid, pct) {
  const out = await runScript(['-Action', 'setSessionVolume', '-ProcessId', String(pid), '-Value', String(Math.round(pct))]);
  return JSON.parse(out);
}

async function setSessionMuted(pid, muted) {
  const out = await runScript(['-Action', 'setSessionMuted', '-ProcessId', String(pid), '-Value', muted ? 'true' : 'false']);
  return JSON.parse(out);
}

module.exports = { listDevices, setDevice, listSessions, setSessionVolume, setSessionMuted };
