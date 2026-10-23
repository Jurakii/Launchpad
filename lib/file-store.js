// Stored files: copies of files and folders dropped on the grid, kept in
// userData\files\<id>\<name> so a tile keeps working after its original is
// moved or deleted. Thumbnails live in files\thumbs\<id>.png. The renderer
// never touches any of this directly - it all goes through IPC in main.js.
//
// The entries themselves are ordinary apps.json items with type 'stored'
// (see normalize() in store.js, which re-derives their target and icon path
// from the id on every load rather than trusting the file).
const fs = require('fs');
const path = require('path');
const electron = require('electron');
const store = require('./store');
const layout = require('./layout');
const { writeFileAtomic } = require('./safe-file');

const DEFAULT_LIMIT = 2 * 1024 ** 3; // 2 GB
const DEFAULT_FILE_LIMIT = 10000; // files per stored folder
const THUMB_SIZE = 256;

// Launchable things are never copied: a copied program loses its config and
// relative paths, and a shortcut's target is what matters. These keep the
// old behavior of a 'path'/'script' entry pointing at the original.
const LINK_EXTENSIONS = ['.exe', '.com', '.msi', '.bat', '.cmd', '.ps1', '.vbs', '.lnk', '.url', '.py', '.js', '.mjs'];

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const thumbsDir = () => path.join(store.filesDir(), 'thumbs');

function assertUuid(id) {
  if (typeof id !== 'string' || !UUID_RE.test(id)) throw new Error(`Not a stored item id: ${id}`);
}

// Only files are linked by extension; folders are always copied (subject to
// the size limit), whatever their name ends in.
function isLinkable(filePath) {
  return LINK_EXTENSIONS.includes(path.extname(filePath).toLowerCase());
}

function tooLarge(size, files, limit, fileLimit) {
  return Object.assign(new Error('Too large to store'), { code: 'tooLarge', size, files, limit, fileLimit });
}

// Total bytes and file count under a folder, skipping symlinks/junctions the
// same way copyDir does. Stops as soon as either limit is passed, so a huge
// folder is cheap to reject.
async function dirSize(dir, limit, fileLimit, acc = { size: 0, files: 0 }) {
  for (const entry of await fs.promises.readdir(dir, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) continue;
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      await dirSize(p, limit, fileLimit, acc);
    } else if (entry.isFile()) {
      acc.size += (await fs.promises.stat(p)).size;
      acc.files++;
    }
    if (acc.size > limit || acc.files > fileLimit) throw tooLarge(acc.size, acc.files, limit, fileLimit);
  }
  return acc;
}

// Recursive copy. Symlinks and junctions are skipped, so a folder can't loop
// or pull in content from outside itself.
async function copyDir(from, to) {
  await fs.promises.mkdir(to, { recursive: true });
  for (const entry of await fs.promises.readdir(from, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) continue;
    const s = path.join(from, entry.name);
    const d = path.join(to, entry.name);
    if (entry.isDirectory()) await copyDir(s, d);
    else if (entry.isFile()) await fs.promises.copyFile(s, d);
  }
}

// Writes thumbs\<id>.png: the shell's thumbnail where there is one (images,
// PDFs, videos), else the file's icon. Folders get no file: the renderer
// draws its own folder glyph, since app.getFileIcon on a folder returns a
// drive icon on Windows and the shell has no thumbnail for an empty one.
async function makeThumbnail(filePath, id, isFolder) {
  const thumbFile = path.join(thumbsDir(), `${id}.png`);
  if (isFolder) {
    await fs.promises.rm(thumbFile, { force: true });
    return { state: 'generic' };
  }
  await fs.promises.mkdir(thumbsDir(), { recursive: true });
  try {
    const img = await electron.nativeImage.createThumbnailFromPath(filePath, { width: THUMB_SIZE, height: THUMB_SIZE });
    if (!img || img.isEmpty()) throw new Error('no thumbnail');
    writeFileAtomic(thumbFile, img.toPNG());
    return { state: 'ready' };
  } catch {}
  try {
    const icon = await electron.app.getFileIcon(filePath, { size: 'large' });
    if (icon && !icon.isEmpty()) writeFileAtomic(thumbFile, icon.toPNG());
  } catch {}
  return { state: 'generic' };
}

// Copies one file or folder into files\<id>\. Never changes the source. On
// any failure the half-written folder is removed and the error rethrown.
async function storeFromPath(sourcePath, { id = store.newId(), limit = DEFAULT_LIMIT, fileLimit = DEFAULT_FILE_LIMIT } = {}) {
  assertUuid(id);
  const src = await fs.promises.stat(sourcePath);
  const isFolder = src.isDirectory();
  if (!src.isFile() && !isFolder) throw new Error('Not a file or folder');

  const size = isFolder ? (await dirSize(sourcePath, limit, fileLimit)).size : src.size;
  if (size > limit) throw tooLarge(size, 1, limit, fileLimit);

  const name = path.basename(sourcePath);
  const dest = path.join(store.filesDir(), id);
  const finalPath = path.join(dest, name);
  try {
    await fs.promises.mkdir(dest, { recursive: true });
    if (isFolder) await copyDir(sourcePath, finalPath);
    else await fs.promises.copyFile(sourcePath, finalPath);
  } catch (err) {
    await fs.promises.rm(dest, { recursive: true, force: true });
    throw err;
  }

  const thumb = await makeThumbnail(finalPath, id, isFolder);
  return {
    id,
    storedName: name,
    target: finalPath,
    storedKind: isFolder ? 'folder' : 'file',
    originalPath: sourcePath,
    size,
    thumbState: thumb.state,
  };
}

// Deletes a stored item's copy and thumbnail. Only ever touches
// files\<uuid>\ and thumbs\<uuid>.png - a bad id must never become a delete
// somewhere else.
async function removeStored(id) {
  assertUuid(id);
  await fs.promises.rm(path.join(store.filesDir(), id), { recursive: true, force: true });
  await fs.promises.rm(path.join(thumbsDir(), `${id}.png`), { force: true });
}

// Display name for a new stored item: the file name without its extension
// (folders keep theirs), with " (2)", " (3)"... when a sibling already uses it.
function displayName(apps, parentId, stored) {
  const ext = stored.storedKind === 'file' ? path.extname(stored.storedName) : '';
  const base = (ext ? stored.storedName.slice(0, -ext.length) : stored.storedName) || stored.storedName;
  const taken = new Set(
    apps.filter((a) => (a.parentId || null) === (parentId || null)).map((a) => String(a.name).toLowerCase())
  );
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) {
    const candidate = `${base} (${n})`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

function newEntry(apps, stored, { parentId = null, gridX = 0, gridY = 0, now = Date.now() } = {}) {
  const occupied = apps
    .filter((a) => (a.parentId || null) === (parentId || null) && typeof a.gridX === 'number' && typeof a.gridY === 'number')
    .map((a) => ({ ...layout.footprintOf(a), x: a.gridX, y: a.gridY }));
  const pos = layout.findNearestFreeCell(occupied, gridX, gridY, 1, 1);
  return store.normalize({
    id: stored.id,
    kind: 'app',
    name: displayName(apps, parentId, stored),
    type: 'stored',
    target: stored.target,
    storedName: stored.storedName,
    storedKind: stored.storedKind,
    originalPath: stored.originalPath,
    size: stored.size,
    thumbState: stored.thumbState,
    args: '',
    cwd: '',
    interpreter: '',
    command: '',
    parentId: parentId || null,
    pinned: false,
    createdAt: now,
    lastLaunched: null,
    launchCount: 0,
    color: null,
    iconIsCustom: false,
    gridX: pos.x,
    gridY: pos.y,
  });
}

function friendlyError(err, sourcePath) {
  const name = path.basename(sourcePath) || sourcePath;
  if (err && err.code === 'tooLarge') {
    return `"${name}" is larger than the file store limit (${formatSize(err.limit)} or ${err.fileLimit.toLocaleString()} files).`;
  }
  if (err && err.code === 'ENOENT') return `"${name}" could not be read. It may have been moved or deleted.`;
  if (err && err.code === 'ENOSPC') return `There isn't enough disk space to store "${name}".`;
  if (err && (err.code === 'EACCES' || err.code === 'EPERM' || err.code === 'EBUSY')) {
    return `"${name}" could not be read. It may be open in another program or need permission.`;
  }
  return `"${name}" could not be stored: ${(err && err.message) || err}`;
}

function formatSize(bytes) {
  if (!(bytes >= 0)) return '';
  const units = ['bytes', 'KB', 'MB', 'GB', 'TB'];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  if (i === 0) return n === 1 ? '1 byte' : `${n} bytes`;
  return `${n.toFixed(n < 10 ? 1 : 0)} ${units[i]}`;
}

// Stores each path in turn and appends an entry for each one that worked to
// `apps` (mutated). A failure on one path is reported in `results` and
// doesn't stop the others; nothing is added for a failed copy.
async function storeMany(apps, paths, { parentId = null, gridX = 0, gridY = 0, limit, fileLimit, now } = {}) {
  const results = [];
  for (const p of paths) {
    try {
      const stored = await storeFromPath(p, { limit, fileLimit });
      const entry = newEntry(apps, stored, { parentId, gridX, gridY, now });
      apps.push(entry);
      results.push({ ok: true, path: p, id: entry.id });
    } catch (err) {
      results.push({ ok: false, path: p, tooLarge: !!(err && err.code === 'tooLarge'), message: friendlyError(err, p) });
    }
  }
  return results;
}

module.exports = {
  LINK_EXTENSIONS,
  DEFAULT_LIMIT,
  DEFAULT_FILE_LIMIT,
  thumbsDir,
  isLinkable,
  assertUuid,
  storeFromPath,
  storeMany,
  makeThumbnail,
  removeStored,
  copyDir,
  formatSize,
};
