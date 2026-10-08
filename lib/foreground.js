const { spawn } = require('child_process');
const readline = require('readline');
const { unpackedPath } = require('./unpacked-path');

const scriptPath = unpackedPath(__dirname, 'foreground.ps1');

let child = null;
let wanted = false;

// Starts the foreground watcher (see foreground.ps1) and calls
// onChange(appFocused) every time another app gains or loses the
// foreground. If PowerShell dies unexpectedly while still wanted, it's
// restarted after a short delay, reporting "no app focused" in between so
// the hotkey is never left stuck disabled.
function start(excludePid, onChange) {
  wanted = true;
  if (child) return;
  child = spawn(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', scriptPath, '-ExcludePid', String(excludePid)],
    { windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] }
  );
  const proc = child;
  readline.createInterface({ input: proc.stdout }).on('line', (line) => {
    const state = line.trim();
    if (state === 'app') onChange(true);
    else if (state === 'none') onChange(false);
  });
  const onGone = () => {
    if (child !== proc) return;
    child = null;
    onChange(false);
    if (wanted) setTimeout(() => wanted && start(excludePid, onChange), 2000);
  };
  proc.on('exit', onGone);
  proc.on('error', onGone);
}

function stop() {
  wanted = false;
  if (!child) return;
  const proc = child;
  child = null;
  try {
    proc.kill();
  } catch {
    // ignore
  }
}

module.exports = { start, stop };
