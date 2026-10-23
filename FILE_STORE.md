# Launchpad File Store: Plan

## Overview

Today, dropping a file from Explorer onto Launchpad calls `quickAddFromPath` (`renderer/renderer.js`), which adds a `path` entry that points at the original file. Move or delete that file and the tile breaks.

This feature adds a **stored file** kind. Dropping a file or folder on the grid copies it into Launchpad's own store, shows a thumbnail on the grid, and opens the stored copy. Launchpad then works like a desktop: files live on the grid, and their originals can move or be deleted without breaking anything.

### Goals
- Drop an image, document, folder or any other file onto the grid and it is stored, with a thumbnail that matches its type.
- Stored items sit in the grid like any other item: they can be moved, grouped into islands, pinned, renamed, searched and deleted.
- Opening a stored file launches it with the default program, the same as `path` items.
- Works offline and survives backup and restore.
- The original file is not touched. Storing is a copy.

### Non-goals (first version)
- Syncing changes back to the original file.
- Editing files inside Launchpad.
- Full desktop features: right-click "New", wallpaper folders, and so on.
- Storing files from other drives in place (we copy everything).
- Thumbnails for every file type. Generic shell icons are the fallback.

### Open decisions
1. **Copy or link by default. Decided:** drops copy by default, except executables and shortcuts, which link as today (see "Which files are linked" below). A context-menu item "Add as link" is still available for anything. Alt-drop forces a link.
2. **Large folders.** Recommended: copy up to a configurable limit (default 2 GB or 10,000 files), and above that offer "Link instead" instead of silently copying. Alternative: always link folders.
3. **Delete semantics.** Recommended: removing a stored item deletes its store copy after a confirm, and the original is never touched. Stored files are then not in the Recycle Bin, so the confirm must say so. Alternative: move the copy to the Recycle Bin via `shell.trashItem`, which is recoverable.
4. **Duplicate drops.** Recommended: a second drop of the same file makes a second copy, named "name (2).ext". Alternative: hash the file and reuse the existing copy.

---

## Data model

### Store layout
```
%APPDATA%\launchpad\
  files\
    <id>\                 one folder per stored item (id = UUID, same as apps.json)
      <original name>     the copied file, or the copied folder's contents
    thumbs\
      <id>.png            thumbnail, 256 px wide max
```

Using one folder per id avoids name clashes and matches the existing `media\<id>\` pattern.

### Entry in `apps.json`
Add a new `type` to the existing app entry shape (§4.1 of `CODEBASE_SUMMARY.md`):

```
{
  id, kind: 'app', parentId, pinned, pinOrder, createdAt, gridX, gridY, color,
  name,
  type: 'stored',          // NEW: stored copy in files\<id>\
  target: '<files>\<id>\<original name>',   // absolute path, derived, not trusted on load
  storedKind: 'file' | 'folder',
  originalPath,            // where it came from, for display and "Add as link"
  size,                    // bytes, for the size limit and the info card
  thumbState: 'ready' | 'generic' | 'pending',
  iconIsCustom: false
}
```

Why reuse `apps.json` rather than a new file: stored items are grid items, so they need the same position, island, pin and search handling. Keeping them out of `apps.json` would mean duplicating all of that. This is the opposite of Media's library, which is separate because it has no grid cells.

`target` is recomputed from `id` and the file name at load time, never trusted from disk, so a hand-edited `apps.json` can't point the launcher at arbitrary paths.

### Thumbnails
- **Images** (png, jpg, gif, webp, bmp, svg): `nativeImage.createThumbnailFromPath(path, { width: 256, height: 256 })`. Verify on Windows first, since Electron's docs list platform support per version.
- **Everything else** (documents, videos, archives, folders): the shell's thumbnail via the same call, falling back to `app.getFileIcon(path, { size: 'large' })`, which `lib/icons.js` already uses.
- **Folders**: a generic folder icon (`folderSVG` already exists in the renderer), or the first image in the folder used as the preview. Start with the generic icon.
- Thumbnails are generated once at store time and saved as `thumbs\<id>.png`. They are regenerated only when the user asks, so the grid never waits on disk reads.

---

### Which files are linked
Launchable items are never copied. They keep today's behavior: a `path`, `script` or `command` entry pointing at the original.

| Extensions | Treated as | Why |
|---|---|---|
| `.exe`, `.com`, `.msi`, `.bat`, `.cmd`, `.ps1`, `.vbs` | Link | Programs and install packages. A copy of an app often breaks its config or its relative paths. |
| `.lnk`, `.url` | Link | Shortcuts. The target is what matters, and `.lnk` is resolved to it. |
| `.py`, `.js`, `.mjs` | Link | Scripts. `quickAddFromPath` already makes these `script` entries that run the original. |
| Everything else, including folders | Copy | Documents, images, media, archives, and folders |

The list lives in one constant in `lib/file-store.js` (`LINK_EXTENSIONS`), so it's easy to extend. Folders are never linked by extension; they follow the size rule above.

## Main process (`lib/file-store.js`, new)

Responsibilities: copy in, thumbnail, remove, size check. The renderer never touches the filesystem; everything goes through IPC, as today.

Pseudo code:

```js
// lib/file-store.js
const STORE_DIR = path.join(app.getPath('userData'), 'files');
const THUMB_DIR = path.join(STORE_DIR, 'thumbs');
const DEFAULT_LIMIT = 2 * 1024 ** 3; // 2 GB, from settings.fileStoreLimit

async function storeFromPath(sourcePath, { id = randomUUID(), limit }) {
  const src = await fs.promises.stat(sourcePath);           // must exist
  if (!src.isFile() && !src.isDirectory()) throw 'Not a file or folder';

  const size = src.isDirectory() ? await dirSize(sourcePath, limit) : src.size;
  // dirSize stops early once it passes `limit`, so a huge folder is cheap to reject
  if (size > limit) throw { code: 'tooLarge', size, limit };

  const dest = path.join(STORE_DIR, id);
  const name = path.basename(sourcePath);
  const finalPath = path.join(dest, name);
  await fs.promises.mkdir(dest, { recursive: true });

  try {
    if (src.isDirectory()) await copyDir(sourcePath, finalPath); // see below
    else await fs.promises.copyFile(sourcePath, finalPath);
  } catch (err) {
    await fs.promises.rm(dest, { recursive: true, force: true }); // no half-copies
    throw err;
  }

  const thumb = await makeThumbnail(finalPath, id, src.isDirectory());
  return { id, name, target: finalPath, storedKind: src.isDirectory() ? 'folder' : 'file',
           originalPath: sourcePath, size, thumbState: thumb.state };
}

async function makeThumbnail(filePath, id, isFolder) {
  if (isFolder) return { state: 'generic' };
  try {
    const img = await nativeImage.createThumbnailFromPath(filePath, { width: 256, height: 256 });
    if (img.isEmpty()) throw new Error('no thumbnail');
    await writeFileAtomic(path.join(THUMB_DIR, id + '.png'), img.toPNG());
    return { state: 'ready' };
  } catch {
    const icon = await app.getFileIcon(filePath, { size: 'large' });
    await writeFileAtomic(path.join(THUMB_DIR, id + '.png'), icon.toPNG());
    return { state: 'generic' };
  }
}

async function removeStored(id) {
  // only ever deletes inside STORE_DIR; the id must be a UUID (same check as media)
  assertUuid(id);
  await fs.promises.rm(path.join(STORE_DIR, id), { recursive: true, force: true });
  await fs.promises.rm(path.join(THUMB_DIR, id + '.png'), { force: true });
}

async function copyDir(from, to) {
  // recursive copy; skip symlinks and junctions to avoid loops and escaping the folder
  for (const entry of await fs.promises.readdir(from, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) continue;
    const s = path.join(from, entry.name), d = path.join(to, entry.name);
    if (entry.isDirectory()) { await fs.promises.mkdir(d, { recursive: true }); await copyDir(s, d); }
    else await fs.promises.copyFile(s, d);
  }
}
```

### Safety rules
- `removeStored` only deletes under `files\`, and only for UUID ids. This is the key guard: a bad id must never turn into a delete of the user's real files.
- Symlinks and junctions are skipped while copying, so a folder can't pull in content from outside.
- A failed copy removes its own half-written folder.
- Source files are only read, never changed or deleted.
- Long paths (over 260 characters) need the `\\?\` prefix on Windows. Test with a deep folder before shipping.

### Store-side settings
`settings.json` gets:
- `fileStoreLimit` (bytes, default 2 GB)
- `fileStoreOnDrop` (`copy` | `link`, default `copy`), matching open decision 1

---

## IPC (`main.js`, `preload.js`)

| Channel | Input | Output |
|---|---|---|
| `files:store` | `{ paths: string[], parentId, x, y }` | decorated apps list (same as `apps:add`) |
| `files:restoreThumb` | `id` | updated entry (regenerates `thumbs\<id>.png`) |
| `apps:add` | unchanged | used by "Add as link" |

`files:store` accepts a list because a multi-file drop should be one call and one UI refresh. Each path is handled in turn; a failure on one file is reported in the result and doesn't stop the others.

```js
ipcMain.handle('files:store', async (_e, { paths, parentId, x, y }) => {
  const store = loadStore();
  const results = [];
  for (const p of paths) {
    try {
      const stored = await fileStore.storeFromPath(p, { limit: settings.fileStoreLimit });
      const pos = layout.findNearestFreeCell(store, parentId, x, y); // same as other drops
      store.push({ ...newEntry(stored), parentId, gridX: pos.x, gridY: pos.y });
      results.push({ ok: true, path: p });
    } catch (err) {
      results.push({ ok: false, path: p, message: friendlyError(err) });
    }
  }
  saveStore(store);
  return { apps: decorate(store), results };
});
```

Not every call needs a new channel: `apps:update`, `apps:remove`, `apps:launch` and the rest already work on any entry. The stored item just needs `remove` to call `fileStore.removeStored(id)`, so `apps:remove` gets one extra step.

---

## Renderer

### Drop handling (`renderer/renderer.js`, around line 1199)
Today the drop handler calls `quickAddFromPath` for each file. The change is to collect the paths and send one `files:store` call, with the drop position so the item lands under the cursor.

```js
document.body.addEventListener('drop', async (e) => {
  if (isInternalDrag(e) || !e.dataTransfer.types.includes('Files')) return;
  e.preventDefault();
  fileDragDepth = 0;
  document.body.classList.remove('file-drag-over');

  const paths = Array.from(e.dataTransfer.files)
    .map((f) => window.launcherAPI.getPathForFile(f))
    .filter(Boolean);
  if (!paths.length) return;

  // Launchable files (LINK_EXTENSIONS) and Alt-drop link; everything else is copied.
  const forceLink = e.altKey || settings.fileStoreOnDrop === 'link';
  const toLink = paths.filter((p) => forceLink || isLinkable(p));
  const toCopy = paths.filter((p) => !toLink.includes(p));

  for (const p of toLink) await quickAddFromPath(p);      // existing behavior

  if (toCopy.length) {
    const { apps, results } = await window.launcherAPI.storeFiles({
      paths: toCopy, parentId: currentFolderId, x: e.clientX, y: e.clientY,
    });
    refreshFromServer(apps);
    showStoreResults(results);                // one line per failure, not one alert per file
  }
});
```

### Tile rendering (`buildTile`, `renderer/renderer.js:516`)
- A stored entry with `thumbState: 'ready'` shows its thumbnail as the tile image (the same slot the custom icon uses), with the name below.
- `generic` shows the shell icon, as today.
- A small badge marks stored items, so they can be told apart from links at a glance. Keep it subtle.
- If `thumbState` is `pending` (after a restore, before regeneration), show the generic icon and request `files:restoreThumb` in the background.

### Opening
- `apps:launch` for `type: 'stored'` is the same as `type: 'path'`: `shell.openPath(target)`. Files open with their default app, folders in Explorer.
- Recent and search use the same entry, so they need no changes.

### Context menu additions
- **Show original location** (opens `originalPath`'s folder in Explorer, when it still exists)
- **Add as link** (for a stored item: creates a new `path` entry pointing at `originalPath`)
- **Refresh thumbnail**

### Info card
Show `storedKind`, `size`, `originalPath` and when it was stored. Fits the existing item info, if there is one; otherwise a small section in the rename dialog.

---

## Backup and restore (`main.js` ~line 1673, `lib/media/backup.js`, `CODEBASE_SUMMARY.md` §3.5)

Stored files are the user's data, so they must be in backups.

- **Export:** the backup currently embeds icons as base64. A stored file can be large, so keep it out of the JSON and write a sibling folder instead, or add it as a zip. Recommended: a `files/` folder next to the config export, and tell the user to copy it, the same way the Media artwork note works in `README.md`.
- **Import:** entries with `type: 'stored'` whose `files\<id>\` folder is missing are kept as entries with `missing: true`, with a clear message. They are not dropped silently.
- Bump the backup `version` only if the JSON changes shape. Adding `type: 'stored'` to entries is an additive change, so older builds should show these items as broken links, not crash. Confirm that in a test.

---

## Edge cases

| Case | Behavior |
|---|---|
| Source deleted before the drop finishes | `stat` fails; report "could not be read" for that file |
| Drop of a shortcut `.lnk` or a program (`.exe`, `.bat`, etc.) | Linked, not copied (see "Which files are linked") |
| Drop of a huge folder | Rejected over the limit, with an offer to link instead |
| Disk full mid-copy | Half-copy removed, error reported, nothing added to `apps.json` |
| Store folder deleted by hand | Entry shows `missing`; "Remove" clears the entry |
| Same file dropped twice | Second copy, named "(2)" (open decision 4) |
| Drop into an island | `parentId` is the island; the cell is found inside it |
| Drop on the taskbar | Pins it, like a drag-to-pin. Not stored in this version. |
| Media window open | The drop goes to the main window only |
| Read-only store (`apps.json` unreadable) | `files:store` refuses before copying anything, so no orphan copies |

The last row matters: the existing store already goes read-only on a damaged `apps.json` (§4.1). Check that before copying, otherwise a file would be copied with no entry pointing to it.

---

## Tests (`test/file-store.test.js`, new)

Following the existing `node:test` setup in `test/` and the `helpers.js` temp `userData` stub:
1. Copy a file: bytes match, original unchanged.
2. Copy a folder with nested files and a symlink: nested files copied, symlink skipped.
3. Size limit: a folder over the limit is rejected and nothing is written.
4. Copy failure: partial folder is removed.
5. `removeStored` refuses non-UUID ids and never touches a path outside `files\`.
6. Thumbnail: image gives `ready`; a folder gives `generic`; a file with no thumbnail falls back to `generic`.
7. `files:store` with two paths, one bad: one entry added, one failure reported.
8. Long path (over 260 characters) copies on Windows.

Manual checks on Windows: drop a PNG, a PDF, a folder, a `.lnk`, and a multi-file selection; delete the original and confirm the stored copy still opens; restart and confirm thumbnails are still there.

---

## Build order

1. `lib/file-store.js` with copy, remove and the UUID guard, plus tests (no UI yet).
2. Thumbnail generation, checked on Windows for PNG, PDF and folder.
3. `files:store` IPC and the `preload.js` method.
4. Drop handler change and tile rendering with thumbnails and the stored badge.
5. Context menu items: Show original location, Add as link, Refresh thumbnail.
6. Remove-with-confirm, then the "Recycle Bin" decision (open decision 3).
7. Backup export and import handling, with the `missing` state.
8. Update `README.md` (Features and Your data) and `CODEBASE_SUMMARY.md` (§3.4 channels, §4.1 entry shape, §4.2 settings, §4.1 file table) in the same change, as the codebase summary asks.

## Risks

- **Disk use:** copying doubles storage for anything dropped twice. The limit and the link option are the mitigation.
- **Thumbnail API behavior:** `createThumbnailFromPath` support differs by Electron version and file type. Step 2 exists to find this early.
- **Data loss on delete:** store copies skip the Recycle Bin. The confirm and open decision 3 are the mitigation.
- **Backup size:** the file store can make backups large. The sibling-folder approach keeps the JSON small.
