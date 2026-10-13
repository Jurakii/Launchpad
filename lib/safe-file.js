// Crash-safe file helpers for the userData JSON files (apps.json,
// settings.json). fs.writeFileSync truncates the target before writing, so a
// crash or power cut mid-save leaves a half-written file that no longer
// parses - which is how apps.json used to end up "empty" on the next start.
const fs = require('fs');
const path = require('path');

// Defender / the search indexer briefly hold a freshly written file open,
// which makes the rename-over fail with one of these for a few ms.
const TRANSIENT_RENAME_ERRORS = new Set(['EPERM', 'EACCES', 'EBUSY']);
const RENAME_RETRIES = 5;
const RENAME_RETRY_DELAY_MS = 25;

function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// Writes to <file>.tmp, flushes it to disk, then renames it over <file>.
// The rename replaces the file in one step on NTFS, so readers see either
// the old contents or the new ones, never a partial write.
function writeFileAtomic(file, data) {
  const tmp = `${file}.tmp`;
  const fd = fs.openSync(tmp, 'w');
  try {
    fs.writeSync(fd, data, null, 'utf-8');
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }

  for (let attempt = 0; ; attempt++) {
    try {
      fs.renameSync(tmp, file);
      return;
    } catch (err) {
      if (!TRANSIENT_RENAME_ERRORS.has(err.code) || attempt >= RENAME_RETRIES) {
        try {
          fs.unlinkSync(tmp);
        } catch {}
        throw err;
      }
      sleepSync(RENAME_RETRY_DELAY_MS);
    }
  }
}

// Where to keep an unreadable data file so it isn't lost, e.g.
// apps.json -> apps.corrupt-2026-10-13T09-30-00-000Z.json (same folder).
function corruptCopyPath(file) {
  const ext = path.extname(file);
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  return path.join(path.dirname(file), `${path.basename(file, ext)}.corrupt-${stamp}${ext}`);
}

module.exports = { writeFileAtomic, corruptCopyPath };
