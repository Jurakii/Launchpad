// Runs Launchpad 1.2.2's real lib/ modules under plain Node for the fixture
// scripts: `electron` is replaced by a stub whose app.getPath() returns the
// folder given to useUserData() (as test/helpers.js does), and each module is
// required fresh so its in-memory state belongs to that folder.
//
// main.js can't be required outside Electron (it opens windows, the tray and
// IPC at load), so its backup export and import are reproduced below from
// main.js `ipcMain.handle('backup:export' ...)` and `('backup:import' ...)`,
// line for line, minus the dialogs (the path is an argument), the organizer
// call and the renderer decoration. Keep them in step if main.js changes.
const fs = require('fs');
const path = require('path');

const repo = path.resolve(__dirname, '..', '..', '..', '..');
let userData = null;

const electronPath = require.resolve('electron', { paths: [repo] });
require.cache[electronPath] = {
  id: electronPath,
  filename: electronPath,
  loaded: true,
  exports: { app: { getPath: () => userData } },
};

function freshLib(name) {
  for (const key of Object.keys(require.cache)) if (key.startsWith(path.join(repo, 'lib'))) delete require.cache[key];
  return require(path.join(repo, 'lib', name));
}

// Points the modules at `dir` and hands back fresh copies of them.
function useUserData(dir) {
  userData = dir;
  fs.mkdirSync(dir, { recursive: true });
  const store = freshLib('store');
  return {
    store,
    layouts: require(path.join(repo, 'lib', 'layouts')),
    settings: require(path.join(repo, 'lib', 'settings')),
    defaults: require(path.join(repo, 'lib', 'defaults')),
    fileStore: require(path.join(repo, 'lib', 'file-store')),
    library: require(path.join(repo, 'lib', 'media', 'library')),
    players: require(path.join(repo, 'lib', 'media', 'players')),
    controller: require(path.join(repo, 'lib', 'media', 'controller')),
    mediaBackup: require(path.join(repo, 'lib', 'media', 'backup')),
  };
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function storedFilesFolderFor(backupFile) {
  return `${path.basename(backupFile, path.extname(backupFile))}-files`;
}

// main.js backup:export. `version` 2 or a bare array are the older formats,
// built from the same data (see generate.js).
async function exportBackup(m, filePath) {
  const { store, layouts, mediaBackup, fileStore } = m;
  const apps = store.load().map((a) => {
    const entry = { ...a };
    if (a.iconPath && fs.existsSync(a.iconPath)) {
      entry.iconData = fs.readFileSync(a.iconPath).toString('base64');
    }
    delete entry.iconPath;
    return entry;
  });
  const bundle = { version: 3, apps, layouts: layouts.exportData(), ...mediaBackup.exportData() };

  const stored = apps.filter((a) => a.type === 'stored');
  let storedFilesDir = null;
  const storedMissing = [];
  if (stored.length) {
    bundle.storedFilesFolder = storedFilesFolderFor(filePath);
    storedFilesDir = path.join(path.dirname(filePath), bundle.storedFilesFolder);
    for (const a of stored) {
      const src = path.join(store.filesDir(), a.id);
      if (!fs.existsSync(src)) {
        storedMissing.push(a.name);
        continue;
      }
      const dest = path.join(storedFilesDir, a.id);
      await fs.promises.rm(dest, { recursive: true, force: true });
      await fileStore.copyDir(src, dest);
    }
  }
  fs.writeFileSync(filePath, JSON.stringify(bundle, null, 2), 'utf-8');
  return { ok: true, path: filePath, storedFilesDir, storedCount: stored.length, storedMissing };
}

// main.js backup:import.
async function importBackup(m, filePath) {
  const { store, layouts, mediaBackup, fileStore } = m;
  let bundle = null;
  try {
    bundle = JSON.parse(fs.readFileSync(filePath, 'utf-8'));
  } catch {}
  const appList = Array.isArray(bundle) ? bundle : bundle && Array.isArray(bundle.apps) ? bundle.apps : null;
  if (!appList) throw new Error('That file is not a valid Launchpad backup.');
  const rawEntries = appList.filter((e) => e && typeof e === 'object');

  const idMap = new Map();
  const ids = rawEntries.map((e) => {
    const keep = typeof e.id === 'string' && UUID_RE.test(e.id) && !idMap.has(e.id);
    const id = keep ? e.id : store.newId();
    if (typeof e.id === 'string' && !idMap.has(e.id)) idMap.set(e.id, id);
    return id;
  });
  const entries = rawEntries.map(({ iconData, ...rest }, i) => ({
    iconData,
    entry: store.normalize({ ...rest, id: ids[i] }),
  }));
  const islandIds = new Set(entries.filter(({ entry }) => entry.kind === 'island').map(({ entry }) => entry.id));
  for (const { entry } of entries) {
    const parentId = entry.parentId ? idMap.get(entry.parentId) : null;
    entry.parentId = parentId && islandIds.has(parentId) ? parentId : null;
  }

  const backupDir = path.dirname(filePath);
  const folderName =
    !Array.isArray(bundle) && typeof bundle.storedFilesFolder === 'string' && bundle.storedFilesFolder
      ? path.basename(bundle.storedFilesFolder)
      : storedFilesFolderFor(filePath);
  const storedFilesDir = path.join(backupDir, folderName);
  const storedMissing = [];
  for (let i = 0; i < entries.length; i++) {
    const { entry } = entries[i];
    if (entry.type !== 'stored') continue;
    const oldId = rawEntries[i].id;
    const src = typeof oldId === 'string' && UUID_RE.test(oldId) ? path.join(storedFilesDir, oldId) : null;
    const dest = path.join(store.filesDir(), entry.id);
    if (entry.target && fs.existsSync(entry.target)) continue;
    if (src && fs.existsSync(src)) {
      try {
        await fs.promises.rm(dest, { recursive: true, force: true });
        await fileStore.copyDir(src, dest);
        continue;
      } catch (err) {
        console.error(`Failed to restore stored file ${entry.name}:`, err);
      }
    }
    storedMissing.push(entry.name);
  }

  const restored = entries.map(({ iconData, entry }) => {
    if (entry.type === 'stored') {
      if (typeof iconData === 'string' && iconData) {
        fs.mkdirSync(fileStore.thumbsDir(), { recursive: true });
        fs.writeFileSync(entry.iconPath, Buffer.from(iconData, 'base64'));
      } else if (entry.storedKind === 'file') {
        entry.thumbState = 'pending';
      }
      return entry;
    }
    if (typeof iconData === 'string' && iconData) {
      const iconPath = path.join(store.iconsDir(), `${entry.id}.png`);
      fs.writeFileSync(iconPath, Buffer.from(iconData, 'base64'));
      entry.iconPath = iconPath;
    } else {
      entry.iconPath = null;
    }
    return entry;
  });

  store.replaceAll(restored);

  let layoutsError = null;
  if (!Array.isArray(bundle) && bundle.layouts) {
    try {
      layouts.importData(bundle.layouts);
    } catch (err) {
      layoutsError = err.message;
    }
  }
  let mediaError = null;
  try {
    mediaBackup.importData(bundle);
  } catch (err) {
    mediaError = err.message;
  }
  return { ok: true, apps: restored, layoutsError, mediaError, storedMissing };
}

// Every file under `dir`, relative, with forward slashes, sorted.
function listFiles(dir, prefix = '') {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const rel = prefix + e.name;
    if (e.isDirectory()) out.push(...listFiles(path.join(dir, e.name), `${rel}/`));
    else out.push(rel);
  }
  return out.sort();
}

module.exports = { repo, useUserData, exportBackup, importBackup, listFiles };
