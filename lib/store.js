const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { app } = require('electron');
const layout = require('./layout');

const dataDir = () => app.getPath('userData');
const appsFile = () => path.join(dataDir(), 'apps.json');
const iconsDir = () => path.join(dataDir(), 'icons');

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
  return entry;
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
    const occupied = [];
    for (const entry of siblings) {
      const fp = layout.footprintOf(entry);
      if (typeof entry.gridX === 'number' && typeof entry.gridY === 'number') {
        occupied.push({ ...fp, x: entry.gridX, y: entry.gridY });
        continue;
      }
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

function load() {
  ensureDirs();
  let apps;
  try {
    const raw = fs.readFileSync(appsFile(), 'utf-8');
    const parsed = JSON.parse(raw);
    apps = Array.isArray(parsed) ? parsed.map(normalize) : [];
  } catch {
    apps = [];
  }

  let dirty = assignMissingPositions(apps);
  if (assignMissingPinOrders(apps)) dirty = true;
  if (dirty) save(apps);
  return apps;
}

function save(apps) {
  ensureDirs();
  fs.writeFileSync(appsFile(), JSON.stringify(apps, null, 2), 'utf-8');
}

function newId() {
  return crypto.randomUUID();
}

module.exports = {
  load,
  save,
  newId,
  iconsDir,
  dataDir,
  appsFile,
  normalize,
  assignMissingPositions,
  nextPinOrder,
};
