// Small app-level preferences that live outside the apps.json list (e.g.
// window behavior) - needs to be readable synchronously by main.js before
// any renderer exists, so it can't live in the renderer's localStorage.
const fs = require('fs');
const path = require('path');
const { app } = require('electron');

const settingsFile = () => path.join(app.getPath('userData'), 'settings.json');

function load() {
  try {
    const parsed = JSON.parse(fs.readFileSync(settingsFile(), 'utf-8'));
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

function save(patch) {
  const merged = { ...load(), ...patch };
  fs.writeFileSync(settingsFile(), JSON.stringify(merged, null, 2), 'utf-8');
  return merged;
}

module.exports = { load, save };
