const path = require('path');
const fs = require('fs');
const { app, BrowserWindow, ipcMain, dialog, nativeImage } = require('electron');

const store = require('./lib/store');
const icons = require('./lib/icons');
const launcher = require('./lib/launcher');

let mainWindow = null;

function decorate(apps) {
  return apps.map((a) => ({ ...a, iconDataUrl: icons.iconDataUrl(a.iconPath) }));
}

function createWindow() {
  const iconPath = path.join(__dirname, 'build', 'icon.ico');
  mainWindow = new BrowserWindow({
    width: 960,
    height: 680,
    minWidth: 640,
    minHeight: 420,
    icon: fs.existsSync(iconPath) ? iconPath : undefined,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  mainWindow.setMenuBarVisibility(false);
  mainWindow.loadFile(path.join(__dirname, 'renderer', 'index.html'));
}

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });

  app.whenReady().then(createWindow);

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit();
  });

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
}

function inferTypeFromExt(target) {
  const ext = path.extname(target).toLowerCase();
  if (ext === '.py') return { type: 'script', interpreter: 'python' };
  if (ext === '.js' || ext === '.mjs') return { type: 'script', interpreter: 'node' };
  return { type: 'path', interpreter: '' };
}

ipcMain.handle('apps:list', () => decorate(store.load()));

function descendantFolderIds(apps, folderId) {
  const ids = new Set([folderId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const a of apps) {
      if (a.kind === 'folder' && ids.has(a.parentId) && !ids.has(a.id)) {
        ids.add(a.id);
        grew = true;
      }
    }
  }
  return ids;
}

ipcMain.handle('apps:add', async (_e, formData) => {
  const apps = store.load();
  const id = store.newId();
  const entry = {
    id,
    kind: 'app',
    name: formData.name || 'Untitled',
    type: formData.type,
    target: formData.target || '',
    args: formData.args || '',
    cwd: formData.cwd || '',
    interpreter: formData.interpreter || '',
    command: formData.command || '',
    parentId: formData.parentId || null,
    pinned: false,
    createdAt: Date.now(),
    lastLaunched: null,
    iconPath: null,
  };

  if (formData.customIconPath) {
    entry.iconPath = icons.saveCustomIcon(id, formData.customIconPath);
  } else if (entry.target) {
    entry.iconPath = await icons.extractAndSaveIcon(id, entry.target);
  }

  apps.push(entry);
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('apps:update', async (_e, { id, formData }) => {
  const apps = store.load();
  const idx = apps.findIndex((a) => a.id === id);
  if (idx === -1) return decorate(apps);

  const entry = apps[idx];
  Object.assign(entry, {
    name: formData.name || entry.name,
    type: formData.type,
    target: formData.target || '',
    args: formData.args || '',
    cwd: formData.cwd || '',
    interpreter: formData.interpreter || '',
    command: formData.command || '',
  });

  if (formData.customIconPath) {
    entry.iconPath = icons.saveCustomIcon(id, formData.customIconPath);
  } else if (formData.refreshIcon && entry.target) {
    entry.iconPath = (await icons.extractAndSaveIcon(id, entry.target)) || entry.iconPath;
  }

  apps[idx] = entry;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('apps:remove', (_e, id) => {
  const all = store.load();
  const target = all.find((a) => a.id === id);
  // Removing a folder doesn't delete what's inside it - bump its contents
  // up to where the folder itself lived.
  if (target && target.kind === 'folder') {
    for (const a of all) {
      if (a.parentId === id) a.parentId = target.parentId;
    }
  }
  const apps = all.filter((a) => a.id !== id);
  store.save(apps);
  const iconFile = path.join(store.iconsDir(), `${id}.png`);
  if (fs.existsSync(iconFile)) fs.unlinkSync(iconFile);
  return decorate(apps);
});

ipcMain.handle('folder:add', (_e, { name, parentId }) => {
  const apps = store.load();
  const entry = {
    id: store.newId(),
    kind: 'folder',
    name: name || 'New Folder',
    parentId: parentId || null,
    createdAt: Date.now(),
  };
  apps.push(entry);
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:rename', (_e, { id, name }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry && name && name.trim()) entry.name = name.trim();
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:move', (_e, { id, parentId }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (!entry) return decorate(apps);

  if (entry.kind === 'folder' && parentId) {
    const blocked = descendantFolderIds(apps, entry.id);
    if (blocked.has(parentId)) return decorate(apps); // would create a cycle
  }
  entry.parentId = parentId || null;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:togglePin', (_e, id) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry) entry.pinned = !entry.pinned;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('apps:launch', async (_e, id) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (!entry) throw new Error('App not found');
  if (entry.kind === 'folder') throw new Error('That is a folder, not an app');
  await launcher.launch(entry);
  entry.lastLaunched = Date.now();
  store.save(apps);
  return { ok: true };
});

ipcMain.handle('dialog:pickTarget', async (_e, kind) => {
  // Windows can't show a combined file+folder picker with files visible - it
  // silently falls back to a folder-only browser. Keep these as separate
  // dialogs so .exe/.py/etc actually show up.
  const options =
    kind === 'folder'
      ? { properties: ['openDirectory'] }
      : {
          properties: ['openFile'],
          filters: [
            { name: 'Programs & Shortcuts', extensions: ['exe', 'lnk', 'bat', 'cmd'] },
            { name: 'Scripts', extensions: ['py', 'js', 'mjs', 'ps1'] },
            { name: 'All Files', extensions: ['*'] },
          ],
        };

  const res = await dialog.showOpenDialog(mainWindow, options);
  if (res.canceled || !res.filePaths[0]) return null;

  const target = res.filePaths[0];
  const ext = path.extname(target);
  const name = path.basename(target, ext);
  const { type, interpreter } = inferTypeFromExt(target);
  const img = await icons.extractIcon(target);
  const iconDataUrl = img ? img.toDataURL() : null;

  return { target, type, interpreter, name, iconDataUrl };
});

ipcMain.handle('icon:extractPreview', async (_e, targetPath) => {
  if (!targetPath) return null;
  const img = await icons.extractIcon(targetPath);
  return img ? img.toDataURL() : null;
});

ipcMain.handle('dialog:pickIcon', async () => {
  const res = await dialog.showOpenDialog(mainWindow, {
    properties: ['openFile'],
    filters: [{ name: 'Images', extensions: ['png', 'ico', 'jpg', 'jpeg', 'bmp'] }],
  });
  if (res.canceled || !res.filePaths[0]) return null;

  const iconSourcePath = res.filePaths[0];
  const img = nativeImage.createFromPath(iconSourcePath);
  return { path: iconSourcePath, iconDataUrl: img.isEmpty() ? null : img.toDataURL() };
});
