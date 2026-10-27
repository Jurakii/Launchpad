const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { app } = require('electron');
const layout = require('./layout');
const { writeFileAtomic, corruptCopyPath } = require('./safe-file');

const dataDir = () => app.getPath('userData');
const appsFile = () => path.join(dataDir(), 'apps.json');
const backupFile = () => path.join(dataDir(), 'apps.json.bak');
const iconsDir = () => path.join(dataDir(), 'icons');
const filesDir = () => path.join(dataDir(), 'files');

// Set when apps.json exists but won't parse. 'restored' = we rolled back to
// apps.json.bak; 'readOnly' = no usable backup either, so save() refuses
// rather than replacing the user's data with an empty list. Either way the
// unreadable file is copied aside first. The renderer asks for this once
// at startup to tell the user what happened.
let loadIssue = null; // { state: 'restored' | 'readOnly', corruptCopy }
let readOnly = false;

const READ_ONLY_MESSAGE =
  "Launchpad can't save changes because apps.json couldn't be read, and saving now would overwrite it. " +
  'Fix or replace that file (or import a backup from Settings), then try again.';

function ensureDirs() {
  fs.mkdirSync(iconsDir(), { recursive: true });
}

function normalize(entry) {
  // Entries saved before folders/pinning/etc existed are missing these fields.
  if (!entry.kind) entry.kind = 'app';
  if (entry.kind === 'folder') entry.kind = 'island'; // one-time rename migration
  if (entry.parentId === undefined) entry.parentId = null;
  if (entry.pinned === undefined) entry.pinned = false;
  if (entry.launchCount === undefined) entry.launchCount = 0;
  if (entry.color === undefined) entry.color = null;
  if (entry.iconIsCustom === undefined) entry.iconIsCustom = false;
  if (entry.collapsed === undefined) entry.collapsed = true;
  if (entry.kind === 'island') {
    if (entry.w === undefined) entry.w = layout.EXPANDED_ISLAND_W;
    if (entry.h === undefined) entry.h = layout.EXPANDED_ISLAND_H;
    if (entry.showNames === undefined) entry.showNames = false;
  }
  if (entry.type === 'stored') normalizeStored(entry);
  return entry;
}

// A stored file's target and thumbnail always live under files\<id>\ (see
// lib/file-store.js). They're re-derived here rather than trusted from disk,
// so a hand-edited apps.json can't point the launcher at arbitrary paths. A
// name with any path parts in it gets no target at all.
function normalizeStored(entry) {
  const name = typeof entry.storedName === 'string' ? entry.storedName : '';
  const safeName = name && name === path.basename(name) && name !== '.' && name !== '..';
  const safeId = typeof entry.id === 'string' && /^[0-9a-f-]{36}$/i.test(entry.id);
  entry.target = safeName && safeId ? path.join(filesDir(), entry.id, name) : '';
  entry.iconPath = safeId ? path.join(filesDir(), 'thumbs', `${entry.id}.png`) : null;
  entry.iconIsCustom = false;
  if (!['file', 'folder'].includes(entry.storedKind)) entry.storedKind = 'file';
  if (!['ready', 'generic', 'pending'].includes(entry.thumbState)) entry.thumbState = 'pending';
}

// Assigns a free grid cell to any entry that doesn't have one yet - either
// because it predates the freeform canvas, or because it just moved to a
// new parent. Every parent (Home/root, or any given island) is its own
// independent coordinate space, so this groups siblings and places each
// group separately. Mutates `apps` in place, returns whether anything changed.
function assignMissingPositions(apps) {
  let dirty = false;
  const byParent = new Map();
  for (const a of apps) {
    const key = a.parentId || null;
    if (!byParent.has(key)) byParent.set(key, []);
    byParent.get(key).push(a);
  }

  for (const siblings of byParent.values()) {
    siblings.sort((a, b) => (a.createdAt || 0) - (b.createdAt || 0));
    // Every placed sibling counts before anything new is placed. Collecting
    // them along the way (in createdAt order) let an older item that just
    // moved here land on a newer one's cell.
    // Two items already saved on one cell (left by an older version) are
    // pulled apart too: the newer one moves to the nearest free cell.
    const hasCell = (e) => typeof e.gridX === 'number' && typeof e.gridY === 'number';
    const occupied = [];
    for (const entry of siblings.filter(hasCell)) {
      const fp = layout.footprintOf(entry);
      const rect = { ...fp, x: entry.gridX, y: entry.gridY };
      if (occupied.some((o) => layout.rectsOverlap(rect, o))) {
        const pos = layout.findNearestFreeCell(occupied, entry.gridX, entry.gridY, fp.w, fp.h);
        entry.gridX = rect.x = pos.x;
        entry.gridY = rect.y = pos.y;
        dirty = true;
      }
      occupied.push(rect);
    }
    for (const entry of siblings) {
      if (hasCell(entry)) continue;
      const fp = layout.footprintOf(entry);
      const pos = layout.findFirstFreeCell(occupied, fp.w, fp.h);
      entry.gridX = pos.x;
      entry.gridY = pos.y;
      occupied.push({ ...fp, x: pos.x, y: pos.y });
      dirty = true;
    }
  }
  return dirty;
}

// Gives every currently-pinned item a stable taskbar order the first time
// this runs (existing items keep whatever order they already had; anything
// unordered gets appended after, alphabetically). Mutates in place.
function assignMissingPinOrders(apps) {
  const pinned = apps.filter((a) => a.pinned);
  if (pinned.every((a) => typeof a.pinOrder === 'number')) return false;
  pinned
    .sort((a, b) => {
      const ao = typeof a.pinOrder === 'number' ? a.pinOrder : Infinity;
      const bo = typeof b.pinOrder === 'number' ? b.pinOrder : Infinity;
      return ao - bo || a.name.localeCompare(b.name);
    })
    .forEach((entry, i) => {
      entry.pinOrder = i;
    });
  return true;
}

function nextPinOrder(apps) {
  const orders = apps.filter((a) => a.pinned && typeof a.pinOrder === 'number').map((a) => a.pinOrder);
  return orders.length ? Math.max(...orders) + 1 : 0;
}

// Reads and parses an apps list. Filesystem errors (missing, locked) are
// thrown as-is; content that isn't a JSON array is thrown with
// `unparseable` set, so load() can tell "corrupt" apart from "busy".
function readAppsFile(file) {
  const raw = fs.readFileSync(file, 'utf-8');
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    throw Object.assign(new Error(`${path.basename(file)}: ${err.message}`), { unparseable: true });
  }
  if (!Array.isArray(parsed)) {
    throw Object.assign(new Error(`${path.basename(file)} is not a JSON array`), { unparseable: true });
  }
  return parsed.filter((e) => e && typeof e === 'object').map(normalize);
}

// apps.json exists but won't parse. Keep a copy of it, then roll back to
// apps.json.bak if that's readable, otherwise go read-only.
function recoverFromUnreadable(err) {
  if (readOnly) return []; // already copied aside and reported this session

  const corruptCopy = corruptCopyPath(appsFile());
  fs.copyFileSync(appsFile(), corruptCopy);
  console.error(`apps.json is unreadable (${err.message}); kept a copy at ${corruptCopy}`);

  let restored = null;
  try {
    restored = readAppsFile(backupFile());
  } catch {}

  if (restored) {
    writeFileAtomic(appsFile(), JSON.stringify(restored, null, 2));
    loadIssue = { state: 'restored', corruptCopy };
    return restored;
  }
  readOnly = true;
  loadIssue = { state: 'readOnly', corruptCopy };
  return [];
}

function load() {
  ensureDirs();
  let apps;
  try {
    apps = readAppsFile(appsFile());
  } catch (err) {
    if (err.unparseable) {
      apps = recoverFromUnreadable(err);
      if (readOnly) return apps; // nothing to backfill, and save() would refuse
    } else if (err.code === 'ENOENT') {
      apps = []; // first run - nothing saved yet
    } else {
      // Locked or permission-denied: fail this call and leave the file
      // alone. Treating it as empty is what used to wipe it on the next save.
      throw err;
    }
  }
  if (readOnly) {
    // apps.json was fixed, replaced or removed while we were running.
    readOnly = false;
    loadIssue = null;
  }

  let dirty = assignMissingPositions(apps);
  if (assignMissingPinOrders(apps)) dirty = true;
  if (dirty) save(apps);
  return apps;
}

// Copies the current apps.json to apps.json.bak before it gets replaced, so
// a file that later goes bad can be rolled back on the next load. A file
// that no longer parses is never copied - that would destroy the last good
// backup. Failing to back up shouldn't block the save itself.
function backupCurrent() {
  try {
    readAppsFile(appsFile());
  } catch {
    return;
  }
  try {
    fs.copyFileSync(appsFile(), backupFile());
  } catch (err) {
    console.error('Failed to back up apps.json:', err);
  }
}

function write(apps) {
  ensureDirs();
  backupCurrent();
  writeFileAtomic(appsFile(), JSON.stringify(apps, null, 2));
}

function save(apps) {
  if (readOnly) throw new Error(READ_ONLY_MESSAGE);
  write(apps);
}

// Deliberate whole-list replacement (backup import). Allowed even in
// read-only mode: the user is explicitly choosing new data, and the
// unreadable apps.json was already copied aside.
function replaceAll(apps) {
  write(apps);
  readOnly = false;
  loadIssue = null;
}

function isReadOnly() {
  return readOnly;
}

function getLoadIssue() {
  return loadIssue && { ...loadIssue, appsFile: appsFile() };
}

function newId() {
  return crypto.randomUUID();
}

module.exports = {
  load,
  save,
  replaceAll,
  getLoadIssue,
  isReadOnly,
  newId,
  iconsDir,
  filesDir,
  dataDir,
  appsFile,
  normalize,
  assignMissingPositions,
  nextPinOrder,
};
