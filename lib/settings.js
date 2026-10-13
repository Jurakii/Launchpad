// Small app-level preferences that live outside the apps.json list (e.g.
// window behavior) - needs to be readable synchronously by main.js before
// any renderer exists, so it can't live in the renderer's localStorage.
const fs = require('fs');
const path = require('path');
const { app } = require('electron');
const { writeFileAtomic, corruptCopyPath } = require('./safe-file');

const settingsFile = () => path.join(app.getPath('userData'), 'settings.json');

// Throws on filesystem errors other than "doesn't exist yet", so save()
// never merges into {} (and drops every other setting) just because the
// file was briefly locked.
function read() {
  let raw;
  try {
    raw = fs.readFileSync(settingsFile(), 'utf-8');
  } catch (err) {
    if (err.code === 'ENOENT') return {};
    throw err;
  }
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) return parsed;
  } catch {}

  // Unparseable: move it aside rather than letting the next save() quietly
  // replace it. Moved, not copied, so this happens once and the next save
  // starts a fresh file.
  const corruptCopy = corruptCopyPath(settingsFile());
  try {
    fs.renameSync(settingsFile(), corruptCopy);
  } catch (err) {
    throw new Error(`settings.json is unreadable and could not be moved aside: ${err.message}`);
  }
  console.error(`settings.json is unreadable; moved it to ${corruptCopy}`);
  return {};
}

function load() {
  try {
    return read();
  } catch {
    return {}; // locked right now - use defaults for this read only
  }
}

function save(patch) {
  const merged = { ...read(), ...patch };
  writeFileAtomic(settingsFile(), JSON.stringify(merged, null, 2));
  return merged;
}

module.exports = { load, save };
