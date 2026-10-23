// Media's controller settings, kept in settings.json under
// `controller`: the stick dead zone and a button map per pad (keyed by the
// Gamepad id string, so a second, different pad keeps its own). A pad with
// no map uses pad.js's standard (Xbox) mapping.
const settings = require('../settings');

const ACTIONS = ['select', 'back', 'quick', 'customize', 'tabPrev', 'tabNext', 'pageUp', 'pageDown', 'menu', 'up', 'down', 'left', 'right'];
const DEFAULT_DEAD_ZONE = 0.35;
const MAX_PADS = 16;
const MAX_BUTTON = 31;

// The dead zone, clamped to 10–90% in 5% steps.
function cleanDeadZone(value) {
  const n = Number(value);
  if (!Number.isFinite(n)) return DEFAULT_DEAD_ZONE;
  return Math.round(Math.min(0.9, Math.max(0.1, n)) * 20) / 20;
}

// { "<button index>": action }, valid buttons and actions only, and each
// action on one button at most (the first one listed wins).
function cleanMapping(mapping) {
  const out = {};
  const used = new Set();
  if (!mapping || typeof mapping !== 'object' || Array.isArray(mapping)) return out;
  for (const [key, action] of Object.entries(mapping)) {
    const button = Number(key);
    if (!Number.isInteger(button) || button < 0 || button > MAX_BUTTON) continue;
    if (!ACTIONS.includes(action) || used.has(action)) continue;
    used.add(action);
    out[button] = action;
  }
  return out;
}

function sanitize(data) {
  const src = data && typeof data === 'object' ? data : {};
  const pads = {};
  const entries = src.pads && typeof src.pads === 'object' ? Object.entries(src.pads) : [];
  for (const [id, mapping] of entries.slice(0, MAX_PADS)) {
    if (typeof id !== 'string' || !id || id.length > 200) continue;
    const clean = cleanMapping(mapping);
    // A map without select and back would leave the pad unable to get out.
    const actions = Object.values(clean);
    if (actions.includes('select') && actions.includes('back')) pads[id] = clean;
  }
  return { deadZone: cleanDeadZone(src.deadZone), pads };
}

function load() {
  return sanitize(settings.load().controller);
}

function save(data) {
  const clean = sanitize(data);
  settings.save({ controller: clean });
  return clean;
}

module.exports = { load, save, sanitize, ACTIONS, DEFAULT_DEAD_ZONE };
