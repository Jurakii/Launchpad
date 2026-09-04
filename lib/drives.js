const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'drives.ps1');

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

async function listUsbDrives() {
  const out = await runScript(['-Action', 'list']);
  return JSON.parse(out);
}

async function ejectDrive(driveLetter) {
  const out = await runScript(['-Action', 'eject', '-DriveLetter', driveLetter]);
  return JSON.parse(out);
}

module.exports = { listUsbDrives, ejectDrive };
