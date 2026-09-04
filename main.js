const path = require('path');
const fs = require('fs');
const { exec } = require('child_process');
const {
  app,
  BrowserWindow,
  ipcMain,
  dialog,
  nativeImage,
  Tray,
  Menu,
  globalShortcut,
  shell,
} = require('electron');

const store = require('./lib/store');
const icons = require('./lib/icons');
const launcher = require('./lib/launcher');
const layout = require('./lib/layout');
const settings = require('./lib/settings');
const volume = require('./lib/volume');
const audioDevices = require('./lib/audio-devices');
const drives = require('./lib/drives');
const openWindows = require('./lib/windows');
const bluetooth = require('./lib/bluetooth');
const power = require('./lib/power');
const wifi = require('./lib/wifi');

let mainWindow = null;
let tray = null;
let isQuitting = false;
let currentHotkey = null;

const ICON_PATH = path.join(__dirname, 'build', 'icon.ico');
const DEFAULT_HOTKEY = 'CommandOrControl+Space';

// Unregisters whatever hotkey is currently active and registers `accel` in
// its place. On failure (already claimed by another app), leaves the
// previous hotkey working rather than the app silently losing its toggle.
function registerHotkey(accel) {
  if (currentHotkey) {
    try {
      globalShortcut.unregister(currentHotkey);
    } catch {
      // ignore
    }
  }
  const ok = globalShortcut.register(accel, toggleWindow);
  if (ok) {
    currentHotkey = accel;
  } else {
    console.error(`Could not register hotkey "${accel}" (already in use by another app).`);
    if (currentHotkey) globalShortcut.register(currentHotkey, toggleWindow);
  }
  return ok;
}

function decorate(apps) {
  return apps.map((a) => {
    const missing =
      a.kind === 'app' && (a.type === 'path' || a.type === 'script') && a.target
        ? !fs.existsSync(a.target)
        : false;
    return { ...a, iconDataUrl: icons.iconDataUrl(a.iconPath), missing };
  });
}

function showWindow() {
  if (!mainWindow) return;
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.show();
  mainWindow.focus();
}

function toggleWindow() {
  if (!mainWindow) return;
  if (mainWindow.isVisible() && mainWindow.isFocused()) {
    mainWindow.hide();
  } else {
    showWindow();
  }
}

// In dev mode process.execPath is electron.exe itself - point it back at
// this project so "start with Windows" is still meaningful while testing.
// Must be identical for both the set and the get, or the get can never
// match what was actually set and always reads back as disabled.
function loginItemOptions() {
  if (app.isPackaged) return {};
  return { path: process.execPath, args: [path.resolve(__dirname)] };
}

function setAutoLaunch(enable) {
  app.setLoginItemSettings({ openAtLogin: enable, ...loginItemOptions() });
}

function getAutoLaunchEnabled() {
  return app.getLoginItemSettings(loginItemOptions()).openAtLogin;
}

function updateTrayMenu() {
  if (!tray) return;
  const autoLaunchEnabled = getAutoLaunchEnabled();
  const menu = Menu.buildFromTemplate([
    { label: 'Show Launchpad', click: showWindow },
    { type: 'separator' },
    {
      label: 'Start with Windows',
      type: 'checkbox',
      checked: autoLaunchEnabled,
      click: (item) => {
        setAutoLaunch(item.checked);
        updateTrayMenu();
      },
    },
    { type: 'separator' },
    {
      label: 'Quit',
      click: () => {
        isQuitting = true;
        app.quit();
      },
    },
  ]);
  tray.setContextMenu(menu);
}

function createTray() {
  tray = new Tray(fs.existsSync(ICON_PATH) ? ICON_PATH : nativeImage.createEmpty());
  tray.setToolTip('Launchpad');
  tray.on('click', toggleWindow);
  updateTrayMenu();
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 960,
    height: 680,
    minWidth: 640,
    minHeight: 420,
    fullscreen: !!settings.load().startFullscreen,
    icon: fs.existsSync(ICON_PATH) ? ICON_PATH : undefined,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  mainWindow.setMenuBarVisibility(false);
  mainWindow.loadFile(path.join(__dirname, 'renderer', 'index.html'));

  // Closing the window (the X button) minimizes to tray instead of quitting by
  // default - that's the whole point of the tray icon + global hotkey combo -
  // but the user can turn that off in Settings so the X button fully quits.
  mainWindow.on('close', (e) => {
    const minimizeToTray = settings.load().minimizeToTrayOnClose !== false;
    if (!isQuitting && minimizeToTray) {
      e.preventDefault();
      mainWindow.hide();
    }
  });

  // The fullscreen toolbar button needs to reflect reality even when
  // fullscreen is toggled some other way (F11, Esc, OS window snap), so push
  // state changes to the renderer instead of only answering on request.
  mainWindow.on('enter-full-screen', () => mainWindow.webContents.send('fullscreen-changed', true));
  mainWindow.on('leave-full-screen', () => mainWindow.webContents.send('fullscreen-changed', false));
}

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    showWindow();
  });

  app.on('before-quit', () => {
    isQuitting = true;
  });

  app.whenReady().then(async () => {
    // True first run (apps.json has never existed) - seed the two things
    // every fresh Windows install already has, rather than opening to an
    // empty launcher. Failure here (e.g. icon extraction acting up on some
    // system) should never be able to silently prevent the window from
    // opening at all - that already happened once during development.
    try {
      if (!fs.existsSync(store.appsFile())) {
        store.load(); // ensures the icons/ dir exists before we try to save icons into it
        store.save(await buildDefaultAppEntries());
      }
    } catch (err) {
      console.error('Failed to seed default apps:', err);
    }
    createWindow();
    createTray();
    registerHotkey(settings.load().hotkey || DEFAULT_HOTKEY);
  });

  app.on('will-quit', () => {
    globalShortcut.unregisterAll();
  });

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
    else showWindow();
  });
}

// Resolves a .lnk shortcut to what it actually points at, so "add this
// shortcut" ends up storing the real app instead of the shortcut file.
function resolveShortcutTarget(filePath) {
  if (path.extname(filePath).toLowerCase() !== '.lnk') return null;
  try {
    const info = shell.readShortcutLink(filePath);
    return info && info.target ? info : null;
  } catch {
    return null; // not a readable/valid shortcut - leave it as-is
  }
}

function applyShortcutResolution(entry) {
  if (entry.type !== 'path' || !entry.target) return;
  const info = resolveShortcutTarget(entry.target);
  if (!info) return;

  const targetDir = path.dirname(info.target);
  entry.target = info.target;
  if (info.args && info.args.trim()) entry.args = info.args.trim();
  if (info.cwd && path.resolve(info.cwd) !== path.resolve(targetDir)) entry.cwd = info.cwd;
}

function autoFetchIcon(id, entry) {
  return entry.type === 'url' ? icons.fetchAndSaveFavicon(id, entry.target) : icons.extractAndSaveIcon(id, entry.target);
}

// File Explorer and Recycle Bin - the two things every fresh Windows
// install already has, so they're a sane default rather than an empty
// launcher. Recycle Bin isn't a real file (it's a virtual shell folder), so
// it's a custom command opening the special shell namespace path, with its
// icon pulled from the matching CLSID rather than a target file.
const EXPLORER_PATH = 'C:\\Windows\\explorer.exe';
const RECYCLE_BIN_CLSID = '::{645FF040-5081-101B-9F08-00AA002F954E}';

async function buildDefaultAppEntries() {
  const now = Date.now();

  const explorerId = store.newId();
  const explorerEntry = {
    id: explorerId,
    kind: 'app',
    name: 'File Explorer',
    type: 'path',
    target: EXPLORER_PATH,
    args: '',
    cwd: '',
    interpreter: '',
    command: '',
    parentId: null,
    pinned: false,
    createdAt: now,
    lastLaunched: null,
    launchCount: 0,
    color: null,
    iconPath: null,
    iconIsCustom: false,
  };
  if (fs.existsSync(EXPLORER_PATH)) {
    explorerEntry.iconPath = await autoFetchIcon(explorerId, explorerEntry);
  }

  const binId = store.newId();
  const binEntry = {
    id: binId,
    kind: 'app',
    name: 'Recycle Bin',
    type: 'command',
    target: '',
    args: '',
    cwd: '',
    interpreter: '',
    command: `explorer.exe ${RECYCLE_BIN_CLSID}`,
    parentId: null,
    pinned: false,
    createdAt: now + 1,
    lastLaunched: null,
    launchCount: 0,
    color: null,
    iconPath: null,
    iconIsCustom: false,
    // Windows won't hand over its real recycle-bin icon through any path
    // form app.getFileIcon understands (tried the CLSID, shell: URI, the
    // registry-documented imageres.dll,-55 resource, and the raw $Recycle.Bin
    // folder - all just return a generic file/folder icon). The renderer
    // draws a proper trash-can glyph for any item flagged like this instead.
    special: 'recycleBin',
  };

  return [explorerEntry, binEntry];
}

function inferTypeFromExt(target) {
  const ext = path.extname(target).toLowerCase();
  if (ext === '.py') return { type: 'script', interpreter: 'python' };
  if (ext === '.js' || ext === '.mjs') return { type: 'script', interpreter: 'node' };
  return { type: 'path', interpreter: '' };
}

function descendantIslandIds(apps, islandId) {
  const ids = new Set([islandId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const a of apps) {
      if (a.kind === 'island' && ids.has(a.parentId) && !ids.has(a.id)) {
        ids.add(a.id);
        grew = true;
      }
    }
  }
  return ids;
}

// Cell footprints currently occupied among siblings sharing `parentId`
// (null = Home/root, or an island's id), excluding one entry (typically the
// item being placed/repositioned itself). Every parent is its own
// independent coordinate space.
function siblingOccupiedFootprints(apps, parentId, excludeId) {
  const key = parentId || null;
  return apps
    .filter(
      (a) => (a.parentId || null) === key && a.id !== excludeId && typeof a.gridX === 'number' && typeof a.gridY === 'number'
    )
    .map((a) => ({ ...layout.footprintOf(a), x: a.gridX, y: a.gridY }));
}

ipcMain.handle('apps:list', () => decorate(store.load()));

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
    launchCount: 0,
    color: null,
    iconPath: null,
    iconIsCustom: false,
  };
  applyShortcutResolution(entry);

  const pos0 = layout.findFirstFreeCell(siblingOccupiedFootprints(apps, entry.parentId, id), 1, 1);
  entry.gridX = pos0.x;
  entry.gridY = pos0.y;

  if (formData.customIconPath) {
    entry.iconPath = icons.saveCustomIcon(id, formData.customIconPath);
    entry.iconIsCustom = true;
  } else if (entry.target) {
    entry.iconPath = await autoFetchIcon(id, entry);
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
  applyShortcutResolution(entry);

  if (formData.customIconPath) {
    // A deliberately-chosen icon. Never silently overwritten again unless
    // the user explicitly re-picks or clicks "Auto-detect Icon".
    entry.iconPath = icons.saveCustomIcon(id, formData.customIconPath);
    entry.iconIsCustom = true;
  } else if (formData.forceAutoIcon || !entry.iconIsCustom) {
    if (entry.target) entry.iconPath = (await autoFetchIcon(id, entry)) || entry.iconPath;
    entry.iconIsCustom = false;
  }

  apps[idx] = entry;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('apps:remove', (_e, id) => {
  const all = store.load();
  const target = all.find((a) => a.id === id);
  // Removing an island doesn't delete what's inside it - bump its contents
  // up to where the island itself lived.
  if (target && target.kind === 'island') {
    for (const a of all) {
      if (a.parentId === id) {
        a.parentId = target.parentId;
        // Its old position was only valid in the removed island's own
        // coordinate space.
        delete a.gridX;
        delete a.gridY;
      }
    }
  }
  const apps = all.filter((a) => a.id !== id);
  store.assignMissingPositions(apps); // anything just bumped up needs a fresh cell
  store.save(apps);
  const iconFile = path.join(store.iconsDir(), `${id}.png`);
  if (fs.existsSync(iconFile)) fs.unlinkSync(iconFile);
  return decorate(apps);
});

ipcMain.handle('folder:add', (_e, { name, parentId, color }) => {
  const apps = store.load();
  const entry = {
    id: store.newId(),
    kind: 'island',
    name: name || 'New Island',
    parentId: parentId || null,
    color: color || null,
    pinned: false,
    collapsed: true,
    w: layout.EXPANDED_ISLAND_W,
    h: layout.EXPANDED_ISLAND_H,
    createdAt: Date.now(),
  };
  const pos1 = layout.findFirstFreeCell(siblingOccupiedFootprints(apps, entry.parentId, entry.id), 1, 1);
  entry.gridX = pos1.x;
  entry.gridY = pos1.y;
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

ipcMain.handle('item:setColor', (_e, { id, color }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry) entry.color = color || null;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:move', (_e, { id, parentId }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (!entry) return decorate(apps);

  if (entry.kind === 'island' && parentId) {
    const blocked = descendantIslandIds(apps, entry.id);
    if (blocked.has(parentId)) return decorate(apps); // would create a cycle
  }
  if (parentId === entry.id) return decorate(apps); // no-op: can't be its own parent
  if ((parentId || null) !== (entry.parentId || null)) {
    // Old gridX/gridY were only valid in the previous parent's coordinate
    // space - clear them so it gets placed fresh among its new siblings.
    delete entry.gridX;
    delete entry.gridY;
  }
  entry.parentId = parentId || null;
  store.assignMissingPositions(apps);
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:setPosition', (_e, { id, gridX, gridY }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (!entry) return decorate(apps);

  const fp = layout.footprintOf(entry);
  const pos = layout.findNearestFreeCell(
    siblingOccupiedFootprints(apps, entry.parentId, entry.id),
    Math.round(gridX),
    Math.round(gridY),
    fp.w,
    fp.h
  );
  entry.gridX = pos.x;
  entry.gridY = pos.y;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:setCollapsed', (_e, { id, collapsed }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry && entry.kind === 'island') {
    entry.collapsed = !!collapsed;
    // Footprint just changed size (1x1 <-> its expanded WxH) - nudge to the
    // nearest free spot among siblings if it now overlaps one.
    const fp = layout.footprintOf(entry);
    const pos = layout.findNearestFreeCell(
      siblingOccupiedFootprints(apps, entry.parentId, entry.id),
      entry.gridX,
      entry.gridY,
      fp.w,
      fp.h
    );
    entry.gridX = pos.x;
    entry.gridY = pos.y;
  }
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:setShowNames', (_e, { id, showNames }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry && entry.kind === 'island') entry.showNames = !!showNames;
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('item:setSize', (_e, { id, w, h }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry && entry.kind === 'island') {
    entry.w = Math.max(layout.MIN_ISLAND_W, Math.min(layout.MAX_ISLAND_W, Math.round(w)));
    entry.h = Math.max(layout.MIN_ISLAND_H, Math.min(layout.MAX_ISLAND_H, Math.round(h)));
    if (entry.collapsed === false) {
      const fp = layout.footprintOf(entry);
      const pos = layout.findNearestFreeCell(
        siblingOccupiedFootprints(apps, entry.parentId, entry.id),
        entry.gridX,
        entry.gridY,
        fp.w,
        fp.h
      );
      entry.gridX = pos.x;
      entry.gridY = pos.y;
    }
  }
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('apps:detectMissingIcons', async () => {
  const apps = store.load();
  let fixed = 0;
  for (const entry of apps) {
    if (entry.kind !== 'app' || !entry.target) continue;
    const hasIcon = entry.iconPath && fs.existsSync(entry.iconPath);
    if (hasIcon) continue;
    const newPath = await autoFetchIcon(entry.id, entry);
    if (newPath) {
      entry.iconPath = newPath;
      entry.iconIsCustom = false;
      fixed++;
    }
  }
  store.save(apps);
  return { apps: decorate(apps), fixed };
});

ipcMain.handle('apps:restoreDefaults', async () => {
  const apps = store.load();
  const existingNames = new Set(apps.map((a) => a.name.toLowerCase()));
  const defaults = await buildDefaultAppEntries();
  const toAdd = defaults.filter((d) => !existingNames.has(d.name.toLowerCase()));
  apps.push(...toAdd);
  store.assignMissingPositions(apps); // new entries have no gridX/gridY yet - find them a free cell
  store.save(apps);
  return { apps: decorate(apps), added: toAdd.length };
});

ipcMain.handle('system:emptyRecycleBin', () => {
  return new Promise((resolve) => {
    exec('powershell -NoProfile -NonInteractive -Command "Clear-RecycleBin -Force -ErrorAction SilentlyContinue"', (error) => {
      resolve({ ok: !error, error: error ? error.message : null });
    });
  });
});

ipcMain.handle('system:getVolume', async () => {
  try {
    return await volume.getVolume();
  } catch (err) {
    return { volume: 0, muted: false, error: err.message };
  }
});

ipcMain.handle('system:setVolume', async (_e, pct) => {
  try {
    await volume.setVolume(pct);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:setMuted', async (_e, muted) => {
  try {
    await volume.setMuted(muted);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:openNetworkSettings', () => {
  shell.openExternal('ms-settings:network-status');
  return { ok: true };
});

ipcMain.handle('system:listAudioDevices', async () => {
  try {
    return await audioDevices.listDevices();
  } catch (err) {
    return [];
  }
});

ipcMain.handle('system:setAudioDevice', async (_e, deviceId) => {
  try {
    return await audioDevices.setDevice(deviceId);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:listAudioSessions', async () => {
  try {
    return await audioDevices.listSessions();
  } catch (err) {
    return [];
  }
});

ipcMain.handle('system:setSessionVolume', async (_e, pid, pct) => {
  try {
    return await audioDevices.setSessionVolume(pid, pct);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:setSessionMuted', async (_e, pid, muted) => {
  try {
    return await audioDevices.setSessionMuted(pid, muted);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:listUsbDrives', async () => {
  try {
    return await drives.listUsbDrives();
  } catch (err) {
    return [];
  }
});

ipcMain.handle('system:ejectDrive', async (_e, driveLetter) => {
  try {
    return await drives.ejectDrive(driveLetter);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

// Icon extraction is a slow shell call, and the same exe gets asked about
// on every poll while the window list is open - cache per path so it only
// happens once per unique running app.
const windowIconCache = new Map();

async function iconDataUrlForExe(exePath) {
  if (!exePath) return null;
  if (windowIconCache.has(exePath)) return windowIconCache.get(exePath);
  let dataUrl = null;
  try {
    const img = await icons.extractIcon(exePath);
    dataUrl = img ? img.toDataURL() : null;
  } catch {
    dataUrl = null;
  }
  windowIconCache.set(exePath, dataUrl);
  return dataUrl;
}

ipcMain.handle('system:listOpenWindows', async () => {
  try {
    const list = await openWindows.listWindows(process.pid);
    for (const w of list) {
      w.iconDataUrl = await iconDataUrlForExe(w.path);
    }
    return list;
  } catch {
    return [];
  }
});

ipcMain.handle('system:focusWindow', async (_e, handle) => {
  try {
    return await openWindows.focusWindow(handle);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:listBluetoothDevices', async () => {
  try {
    return await bluetooth.listDevices();
  } catch {
    return [];
  }
});

ipcMain.handle('system:disconnectBluetoothDevice', async (_e, instanceId) => {
  try {
    return await bluetooth.disconnectDevice(instanceId);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:shutdown', async () => {
  try {
    await power.shutdown();
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:restart', async () => {
  try {
    await power.restart();
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:sleep', async () => {
  try {
    await power.sleep();
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:listWifiNetworks', async () => {
  try {
    return await wifi.listNetworks();
  } catch {
    return [];
  }
});

ipcMain.handle('system:connectWifi', async (_e, ssid) => {
  try {
    return await wifi.connect(ssid);
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('system:disconnectWifi', async () => {
  try {
    return await wifi.disconnect();
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('settings:getMinimizeToTrayOnClose', () => settings.load().minimizeToTrayOnClose !== false);

ipcMain.handle('settings:setMinimizeToTrayOnClose', (_e, enable) => {
  settings.save({ minimizeToTrayOnClose: !!enable });
  return { ok: true };
});

ipcMain.handle('window:isFullscreen', () => (mainWindow ? mainWindow.isFullScreen() : false));

ipcMain.handle('window:toggleFullscreen', () => {
  if (!mainWindow) return { ok: false, isFullscreen: false };
  mainWindow.setFullScreen(!mainWindow.isFullScreen());
  return { ok: true, isFullscreen: mainWindow.isFullScreen() };
});

ipcMain.handle('item:togglePin', (_e, id) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry) {
    entry.pinned = !entry.pinned;
    if (entry.pinned) entry.pinOrder = store.nextPinOrder(apps);
  }
  store.save(apps);
  return decorate(apps);
});

// Deterministic (not toggle) pin - used for bulk actions where a mixed
// selection needs to end up in one definite state, not have some items
// flip one way and some the other.
ipcMain.handle('item:setPinned', (_e, { id, pinned }) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (entry) {
    const wasPinned = entry.pinned;
    entry.pinned = !!pinned;
    if (entry.pinned && !wasPinned) entry.pinOrder = store.nextPinOrder(apps);
  }
  store.save(apps);
  return decorate(apps);
});

// Reorders the taskbar - `orderedIds` is the full pinned list in the order
// the user wants them to appear.
ipcMain.handle('item:setPinOrder', (_e, orderedIds) => {
  const apps = store.load();
  orderedIds.forEach((id, i) => {
    const entry = apps.find((a) => a.id === id);
    if (entry) entry.pinOrder = i;
  });
  store.save(apps);
  return decorate(apps);
});

// Batch reposition - used for dragging a multi-selection as a group. Each
// update is placed in order against the SAME evolving `apps` array, so
// later items in the batch correctly avoid colliding with earlier ones
// that were just placed.
ipcMain.handle('item:setPositions', (_e, updates) => {
  const apps = store.load();
  for (const { id, gridX, gridY } of updates) {
    const entry = apps.find((a) => a.id === id);
    if (!entry) continue;
    const fp = layout.footprintOf(entry);
    const pos = layout.findNearestFreeCell(
      siblingOccupiedFootprints(apps, entry.parentId, entry.id),
      Math.round(gridX),
      Math.round(gridY),
      fp.w,
      fp.h
    );
    entry.gridX = pos.x;
    entry.gridY = pos.y;
  }
  store.save(apps);
  return decorate(apps);
});

ipcMain.handle('apps:launch', async (_e, id) => {
  const apps = store.load();
  const entry = apps.find((a) => a.id === id);
  if (!entry) throw new Error('App not found');
  if (entry.kind === 'island') throw new Error('That is an island, not an app');
  await launcher.launch(entry);
  entry.lastLaunched = Date.now();
  entry.launchCount = (entry.launchCount || 0) + 1;
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

ipcMain.handle('icon:fetchFaviconPreview', async (_e, url) => {
  if (!url) return null;
  const img = await icons.fetchFavicon(url);
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

ipcMain.handle('settings:getAutoLaunch', () => getAutoLaunchEnabled());

ipcMain.handle('settings:setAutoLaunch', (_e, enable) => {
  setAutoLaunch(enable);
  updateTrayMenu();
  return { ok: true };
});

ipcMain.handle('settings:getStartFullscreen', () => !!settings.load().startFullscreen);

ipcMain.handle('settings:setStartFullscreen', (_e, enable) => {
  settings.save({ startFullscreen: !!enable });
  return { ok: true };
});

ipcMain.handle('settings:getHotkey', () => settings.load().hotkey || DEFAULT_HOTKEY);

ipcMain.handle('app:getVersion', () => app.getVersion());

ipcMain.handle('settings:setHotkey', (_e, accel) => {
  if (!accel) return { ok: false, error: 'No keys given.' };
  const ok = registerHotkey(accel);
  if (ok) settings.save({ hotkey: accel });
  return ok ? { ok: true } : { ok: false, error: `"${accel}" is already in use by another app.` };
});

ipcMain.handle('backup:export', async () => {
  const res = await dialog.showSaveDialog(mainWindow, {
    defaultPath: `launchpad-backup-${new Date().toISOString().slice(0, 10)}.json`,
    filters: [{ name: 'Launchpad Backup', extensions: ['json'] }],
  });
  if (res.canceled || !res.filePath) return { ok: false };

  const apps = store.load();
  const bundle = apps.map((a) => {
    const entry = { ...a };
    if (a.iconPath && fs.existsSync(a.iconPath)) {
      entry.iconData = fs.readFileSync(a.iconPath).toString('base64');
    }
    delete entry.iconPath;
    return entry;
  });
  fs.writeFileSync(res.filePath, JSON.stringify(bundle, null, 2), 'utf-8');
  return { ok: true, path: res.filePath };
});

ipcMain.handle('backup:import', async () => {
  const res = await dialog.showOpenDialog(mainWindow, {
    properties: ['openFile'],
    filters: [{ name: 'Launchpad Backup', extensions: ['json'] }],
  });
  if (res.canceled || !res.filePaths[0]) return { ok: false };

  const raw = fs.readFileSync(res.filePaths[0], 'utf-8');
  const bundle = JSON.parse(raw);
  if (!Array.isArray(bundle)) throw new Error('That file is not a valid Launchpad backup.');

  const restored = bundle.map((rawEntry) => {
    const { iconData, ...rest } = rawEntry;
    const entry = store.normalize({ ...rest });
    if (iconData) {
      const iconPath = path.join(store.iconsDir(), `${entry.id}.png`);
      fs.writeFileSync(iconPath, Buffer.from(iconData, 'base64'));
      entry.iconPath = iconPath;
    } else {
      entry.iconPath = null;
    }
    return entry;
  });

  store.save(restored);
  return { ok: true, apps: decorate(restored) };
});
