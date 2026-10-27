// "Reset Launchpad" (Settings → Danger zone): back to a first run, except
// Media, which is kept. Free of Electron apart from the userData path, so it
// can be tested with a temp folder.
//
// Order matters: the pre-reset copy is written (and fsynced) before anything
// is deleted, and a failure writing it stops the reset. A failure part-way
// through leaves that copy in place, so the reset can always be undone by
// importing it from Settings → Backup.
const fs = require('fs');
const path = require('path');
const { app } = require('electron');
const { writeFileAtomic } = require('./safe-file');

const dataDir = () => app.getPath('userData');

// settings.json keys that belong to Media (§4.2), kept like its own files.
const MEDIA_SETTINGS_KEYS = ['controller', 'libraries'];

// What goes, removed outright (folders recursively). With apps.json gone
// the next start is a real first run and seeds the default apps itself.
// settings.json is rewritten instead, keeping Media's keys.
const REMOVED = ['apps.json', 'apps.json.bak', 'layouts.json', 'icons', 'files', 'thumbs'];

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf-8'));
  } catch {
    return null;
  }
}

function preResetPath(now = new Date()) {
  const stamp = now.toISOString().replace(/[:.]/g, '-');
  return path.join(dataDir(), `pre-reset-${stamp}.json`);
}

// A backup-shaped copy of what the reset will replace. Icons are embedded
// like an export's, so importing it brings them back; stored files' copies
// aren't (they can be gigabytes), and the dialog says to export for those.
function writePreReset(now = new Date()) {
  const dir = dataDir();
  const apps = readJson(path.join(dir, 'apps.json'));
  const list = Array.isArray(apps)
    ? apps.map((a) => {
        const entry = { ...a };
        try {
          if (a && a.iconPath && a.type !== 'stored') entry.iconData = fs.readFileSync(a.iconPath).toString('base64');
        } catch {}
        delete entry.iconPath;
        return entry;
      })
    : [];
  const bundle = {
    version: 3,
    preReset: true,
    apps: list,
    layouts: readJson(path.join(dir, 'layouts.json')) || undefined,
    settings: readJson(path.join(dir, 'settings.json')) || undefined,
  };
  const file = preResetPath(now);
  writeFileAtomic(file, JSON.stringify(bundle, null, 2));
  return file;
}

// Runs the reset; the caller relaunches the app straight after, so no
// in-memory state survives it.
function reset({ now = new Date() } = {}) {
  const dir = dataDir();
  const preReset = writePreReset(now);

  const current = readJson(path.join(dir, 'settings.json')) || {};
  const kept = {};
  for (const key of MEDIA_SETTINGS_KEYS) if (current[key] !== undefined) kept[key] = current[key];
  writeFileAtomic(path.join(dir, 'settings.json'), JSON.stringify(kept, null, 2));

  for (const name of REMOVED) fs.rmSync(path.join(dir, name), { recursive: true, force: true });
  return { ok: true, preReset };
}

module.exports = { reset, writePreReset, MEDIA_SETTINGS_KEYS, REMOVED };
