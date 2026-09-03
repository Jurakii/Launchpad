const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { app } = require('electron');

const dataDir = () => app.getPath('userData');
const appsFile = () => path.join(dataDir(), 'apps.json');
const iconsDir = () => path.join(dataDir(), 'icons');

function ensureDirs() {
  fs.mkdirSync(iconsDir(), { recursive: true });
}

function normalize(entry) {
  // Entries saved before folders/pinning existed are missing these fields.
  if (!entry.kind) entry.kind = 'app';
  if (entry.parentId === undefined) entry.parentId = null;
  if (entry.pinned === undefined) entry.pinned = false;
  return entry;
}

function load() {
  ensureDirs();
  try {
    const raw = fs.readFileSync(appsFile(), 'utf-8');
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.map(normalize) : [];
  } catch {
    return [];
  }
}

function save(apps) {
  ensureDirs();
  fs.writeFileSync(appsFile(), JSON.stringify(apps, null, 2), 'utf-8');
}

function newId() {
  return crypto.randomUUID();
}

module.exports = { load, save, newId, iconsDir, dataDir };
