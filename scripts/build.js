// Runs electron-builder, then always re-embeds the app icon into the
// packaged exe (see fix-exe-icon.js) as a safety net - electron-builder's
// own exit code is ignored here since on some machines it fails late (e.g.
// missing code-signing tools for the NSIS installer step) after the
// win-unpacked build it produces along the way is already complete and
// usable; re-applying the same icon when that didn't happen is harmless.
const path = require('path');
const { spawnSync } = require('child_process');

const electronBuilderBin = path.join(
  __dirname,
  '..',
  'node_modules',
  '.bin',
  process.platform === 'win32' ? 'electron-builder.cmd' : 'electron-builder'
);

// spawnSync must be told shell:true to run a .cmd file directly on Windows -
// Node refuses to do so otherwise (EINVAL), a hardening change from
// https://nodejs.org/en/blog/vulnerability/february-2024-security-releases.
const result = spawnSync(electronBuilderBin, process.argv.slice(2), {
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
if (result.status !== 0) {
  console.log('\nelectron-builder exited non-zero - continuing anyway to icon-patch the unpacked build.\n');
}

require('./fix-exe-icon');
