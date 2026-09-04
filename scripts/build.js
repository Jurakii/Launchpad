// Runs electron-builder, then always re-embeds the app icon into the
// packaged exe (see fix-exe-icon.js for why that's necessary on this
// machine). electron-builder's own exit code is ignored here: on a machine
// without Windows Developer Mode enabled it fails late, while trying to
// prep code-signing tools for the NSIS installer step, but the win-unpacked
// build it produces along the way is already complete and usable.
const path = require('path');
const { spawnSync } = require('child_process');

const electronBuilderBin = path.join(
  __dirname,
  '..',
  'node_modules',
  '.bin',
  process.platform === 'win32' ? 'electron-builder.cmd' : 'electron-builder'
);

const result = spawnSync(electronBuilderBin, process.argv.slice(2), { stdio: 'inherit' });
if (result.status !== 0) {
  console.log('\nelectron-builder exited non-zero - continuing anyway to icon-patch the unpacked build.\n');
}

require('./fix-exe-icon');
