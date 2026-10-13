// Runs lib/ modules outside Electron: swaps the `electron` module for a stub
// whose app.getPath() points at a throwaway temp folder, and hands back a
// freshly required copy of the module so in-memory state doesn't leak
// between tests.
const fs = require('fs');
const os = require('os');
const path = require('path');

let userData = null;
const created = [];
process.on('exit', () => {
  for (const dir of created) fs.rmSync(dir, { recursive: true, force: true });
});

const electronPath = require.resolve('electron');
require.cache[electronPath] = {
  id: electronPath,
  filename: electronPath,
  loaded: true,
  exports: { app: { getPath: () => userData } },
};

function freshUserData() {
  userData = fs.mkdtempSync(path.join(os.tmpdir(), 'launchpad-test-'));
  created.push(userData);
  return userData;
}

function freshRequire(modulePath) {
  const resolved = require.resolve(modulePath);
  delete require.cache[resolved];
  return require(resolved);
}

function listDir(dir) {
  return fs.readdirSync(dir).sort();
}

module.exports = { freshUserData, freshRequire, listDir };
