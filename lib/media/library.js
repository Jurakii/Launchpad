// Media Mode's library: games and videos found in the user's library folders.
// Kept in its own library.json rather than apps.json - a few thousand ROMs
// would otherwise each get a Home grid cell, ride along on every apps:list,
// and be embedded in every backup. Same safety behavior as lib/store.js:
// atomic writes, a .bak of the previous version, and a copy-aside +
// restore-or-read-only path when the file won't parse.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { app } = require('electron');
const { writeFileAtomic, corruptCopyPath } = require('../safe-file');

const dataDir = () => app.getPath('userData');
const libraryFile = () => path.join(dataDir(), 'library.json');
const backupFile = () => path.join(dataDir(), 'library.json.bak');
const mediaRoot = () => path.join(dataDir(), 'media');

// Ids double as folder names under media\, so only plain UUIDs are allowed:
// anything else could point a write outside that folder.
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const LIBRARIES = ['games', 'videos'];
const LAUNCH_TYPES = ['profile', 'path', 'url', 'script', 'command'];
const MEDIA_SLOTS = ['cover', 'banner', 'logo', 'screenshot', 'video'];

let loadIssue = null; // { state: 'restored' | 'readOnly', corruptCopy }
let readOnly = false;

const READ_ONLY_MESSAGE =
  "Launchpad can't save library changes because library.json couldn't be read, and saving now would overwrite it. " +
  'Fix or replace that file, then try again.';

function isValidId(id) {
  return typeof id === 'string' && UUID_RE.test(id);
}

function newId() {
  return crypto.randomUUID();
}

// Fills in defaults for missing or unrecognized fields. Does not touch ids;
// fixIds() handles those across the whole list.
function normalize(entry) {
  if (!LIBRARIES.includes(entry.library)) entry.library = 'games';
  if (typeof entry.name !== 'string') entry.name = '';
  if (entry.platform === undefined) entry.platform = null;
  if (!LAUNCH_TYPES.includes(entry.type)) entry.type = 'profile';
  if (entry.profile === undefined) entry.profile = null;
  if (typeof entry.target !== 'string') entry.target = '';
  if (entry.available === undefined) entry.available = true;
  if (typeof entry.launchCount !== 'number') entry.launchCount = 0;
  if (typeof entry.createdAt !== 'number') entry.createdAt = 0;

  const media = entry.media && typeof entry.media === 'object' && !Array.isArray(entry.media) ? entry.media : {};
  entry.media = {};
  for (const slot of MEDIA_SLOTS) {
    const m = media[slot];
    entry.media[slot] = m && typeof m === 'object' && typeof m.path === 'string' ? m : null;
  }
  if (!entry.meta || typeof entry.meta !== 'object' || Array.isArray(entry.meta)) entry.meta = {};
  return entry;
}

// Gives a fresh id to any entry whose id isn't a UUID or repeats an earlier
// one. Mutates in place, returns whether anything changed.
function fixIds(entries) {
  let dirty = false;
  const seen = new Set();
  for (const entry of entries) {
    if (!isValidId(entry.id) || seen.has(entry.id.toLowerCase())) {
      entry.id = newId();
      dirty = true;
    }
    seen.add(entry.id.toLowerCase());
  }
  return dirty;
}

// Filesystem errors are thrown as-is; content that isn't a JSON array is
// thrown with `unparseable` set, so load() can tell "corrupt" from "busy".
function readLibraryFile(file) {
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
  return parsed.filter((e) => e && typeof e === 'object' && !Array.isArray(e)).map(normalize);
}

function recoverFromUnreadable(err) {
  if (readOnly) return []; // already copied aside and reported this session

  const corruptCopy = corruptCopyPath(libraryFile());
  fs.copyFileSync(libraryFile(), corruptCopy);
  console.error(`library.json is unreadable (${err.message}); kept a copy at ${corruptCopy}`);

  let restored = null;
  try {
    restored = readLibraryFile(backupFile());
  } catch {}

  if (restored) {
    fixIds(restored);
    writeFileAtomic(libraryFile(), JSON.stringify(restored, null, 2));
    loadIssue = { state: 'restored', corruptCopy };
    return restored;
  }
  readOnly = true;
  loadIssue = { state: 'readOnly', corruptCopy };
  return [];
}

function load() {
  let entries;
  try {
    entries = readLibraryFile(libraryFile());
  } catch (err) {
    if (err.unparseable) {
      entries = recoverFromUnreadable(err);
      if (readOnly) return entries;
    } else if (err.code === 'ENOENT') {
      entries = []; // no library yet
    } else {
      // Locked or permission-denied: fail this call, never treat it as empty.
      throw err;
    }
  }
  if (readOnly) {
    // library.json was fixed, replaced or removed while we were running.
    readOnly = false;
    loadIssue = null;
  }

  if (fixIds(entries)) save(entries);
  return entries;
}

// Copies the current library.json to .bak before it is replaced. A file
// that no longer parses is never copied over the last good backup.
function backupCurrent() {
  try {
    readLibraryFile(libraryFile());
  } catch {
    return;
  }
  try {
    fs.copyFileSync(libraryFile(), backupFile());
  } catch (err) {
    console.error('Failed to back up library.json:', err);
  }
}

function write(entries) {
  fs.mkdirSync(dataDir(), { recursive: true });
  backupCurrent();
  writeFileAtomic(libraryFile(), JSON.stringify(entries, null, 2));
}

function save(entries) {
  if (readOnly) throw new Error(READ_ONLY_MESSAGE);
  write(entries);
}

// Deliberate whole-list replacement (backup import). Allowed in read-only
// mode: the unreadable file was already copied aside.
function replaceAll(entries) {
  const list = entries.filter((e) => e && typeof e === 'object' && !Array.isArray(e)).map(normalize);
  fixIds(list);
  write(list);
  readOnly = false;
  loadIssue = null;
  return list;
}

function getLoadIssue() {
  return loadIssue && { ...loadIssue, libraryFile: libraryFile() };
}

// The art folder for one entry, or an error for anything but a UUID.
function mediaDirFor(id) {
  if (!isValidId(id)) throw new Error('Invalid library id');
  return path.join(mediaRoot(), id.toLowerCase());
}

module.exports = {
  load,
  save,
  replaceAll,
  getLoadIssue,
  newId,
  isValidId,
  normalize,
  mediaDirFor,
  libraryFile,
  mediaRoot,
  UUID_RE,
  LIBRARIES,
  LAUNCH_TYPES,
  MEDIA_SLOTS,
};
