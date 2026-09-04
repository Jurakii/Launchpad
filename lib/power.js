const { execFile } = require('child_process');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'power.ps1');

function runAction(action) {
  return new Promise((resolve, reject) => {
    execFile(
      'powershell',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', scriptPath, '-Action', action],
      { windowsHide: true },
      (error) => {
        if (error) return reject(error);
        resolve();
      }
    );
  });
}

module.exports = {
  shutdown: () => runAction('shutdown'),
  restart: () => runAction('restart'),
  sleep: () => runAction('sleep'),
};
