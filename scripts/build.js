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

// electron-builder's NSIS artifactName keeps spaces ("Launchpad Setup
// 1.0.1.exe"), but latest.yml - which electron-updater reads at runtime -
// always references the space-free form ("Launchpad-Setup-1.0.1.exe"), since
// that's what its GitHub provider expects to find as the release asset name.
// Uploading the as-built file straight from dist/ silently breaks
// auto-update (GitHub itself mangles the spaces into periods on upload,
// which doesn't match latest.yml either) - so rename it here to the exact
// name latest.yml already commits to, straight after the build produces it.
const fs = require('fs');
const distDir = path.join(__dirname, '..', 'dist');
const latestYmlPath = path.join(distDir, 'latest.yml');
if (fs.existsSync(latestYmlPath)) {
  const yml = fs.readFileSync(latestYmlPath, 'utf-8');
  const wantedNames = new Set(Array.from(yml.matchAll(/^\s*(?:url|path):\s*(.+)$/gm), (m) => m[1].trim()));
  for (const wanted of wantedNames) {
    const actual = wanted.replace(/-/g, ' ');
    if (actual === wanted) continue;
    for (const suffix of ['', '.blockmap']) {
      const actualPath = path.join(distDir, actual + suffix);
      const wantedPath = path.join(distDir, wanted + suffix);
      if (fs.existsSync(actualPath) && !fs.existsSync(wantedPath)) {
        fs.renameSync(actualPath, wantedPath);
        console.log(`renamed to match latest.yml: "${actual}${suffix}" -> "${wanted}${suffix}"`);
      }
    }
  }
}
