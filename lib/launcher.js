const { spawn } = require('child_process');
const { shell } = require('electron');

function splitArgs(str) {
  if (!str) return [];
  const matches = str.match(/"[^"]*"|'[^']*'|\S+/g) || [];
  return matches.map((s) => s.replace(/^['"]|['"]$/g, ''));
}

function detachedSpawn(cmd, args, opts) {
  // Node refuses to spawn a .bat/.cmd file directly on Windows without
  // shell:true (EINVAL) since a 2024 security hardening change, so a target
  // or interpreter that happens to be one (e.g. npm.cmd) needs it even when
  // the caller didn't ask for shell semantics otherwise.
  const needsShell = !!opts.shell || /\.(cmd|bat)$/i.test(cmd);
  const child = spawn(cmd, args, {
    cwd: opts.cwd || undefined,
    shell: needsShell,
    detached: true,
    stdio: 'ignore',
    windowsHide: false,
  });
  child.unref();
}

async function launch(entry) {
  switch (entry.type) {
    case 'path': {
      // Normally a plain open (works for exes and folders alike). If this
      // entry came from a resolved shortcut that carried real launch args or
      // a working directory shell.openPath can't express, spawn directly.
      if (entry.args || entry.cwd) {
        detachedSpawn(entry.target, splitArgs(entry.args), { cwd: entry.cwd });
        return;
      }
      const err = await shell.openPath(entry.target);
      if (err) throw new Error(err);
      return;
    }
    case 'url': {
      await shell.openExternal(entry.target);
      return;
    }
    case 'script': {
      const interpreter = entry.interpreter || 'python';
      detachedSpawn(interpreter, [entry.target, ...splitArgs(entry.args)], {
        cwd: entry.cwd,
      });
      return;
    }
    case 'command': {
      detachedSpawn(entry.command, [], { cwd: entry.cwd, shell: true });
      return;
    }
    default:
      throw new Error(`Unknown launch type: ${entry.type}`);
  }
}

module.exports = { launch };
