const { spawn } = require('child_process');
const { shell } = require('electron');

function splitArgs(str) {
  if (!str) return [];
  const matches = str.match(/"[^"]*"|'[^']*'|\S+/g) || [];
  return matches.map((s) => s.replace(/^['"]|['"]$/g, ''));
}

function detachedSpawn(cmd, args, opts) {
  const child = spawn(cmd, args, {
    cwd: opts.cwd || undefined,
    shell: !!opts.shell,
    detached: true,
    stdio: 'ignore',
    windowsHide: false,
  });
  child.unref();
}

async function launch(entry) {
  switch (entry.type) {
    case 'path': {
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
