// Window layouts for the organizer, kept in <userData>/layouts.json next to
// apps.json. The built-in layouts always exist and are never written to the
// file, so a later version can adjust them; the file only holds the user's
// own layouts plus organizer preferences such as the gap. Nothing in the app
// writes the file yet (the layout editor comes later), so a missing file
// just means "built-ins and defaults".
const fs = require('fs');
const path = require('path');
const { app } = require('electron');
const zones = require('./zones');
const { writeFileAtomic, corruptCopyPath } = require('./safe-file');

const DEFAULT_GAP = 8;
const MAX_GAP = 64;

const zone = (id, name, x, y, w, h) => ({ id, name, x, y, w, h });

const BUILT_IN_LAYOUTS = [
  {
    id: 'halves', name: 'Halves', cols: 2, rows: 1,
    zones: [zone('left', 'Left', 0, 0, 1, 1), zone('right', 'Right', 1, 0, 1, 1)],
  },
  {
    id: 'thirds', name: 'Thirds', cols: 3, rows: 1,
    zones: [zone('left', 'Left', 0, 0, 1, 1), zone('center', 'Center', 1, 0, 1, 1), zone('right', 'Right', 2, 0, 1, 1)],
  },
  {
    id: 'focus', name: 'Focus ⅔', cols: 3, rows: 1,
    zones: [zone('main', 'Main', 0, 0, 2, 1), zone('side', 'Side', 2, 0, 1, 1)],
  },
  {
    id: 'main-stack', name: 'Main + stack', cols: 5, rows: 2,
    zones: [zone('main', 'Main', 0, 0, 3, 2), zone('top', 'Top right', 3, 0, 2, 1), zone('bottom', 'Bottom right', 3, 1, 2, 1)],
  },
  {
    id: 'quarters', name: 'Quarters', cols: 2, rows: 2,
    zones: [
      zone('top-left', 'Top left', 0, 0, 1, 1),
      zone('top-right', 'Top right', 1, 0, 1, 1),
      zone('bottom-left', 'Bottom left', 0, 1, 1, 1),
      zone('bottom-right', 'Bottom right', 1, 1, 1, 1),
    ],
  },
];
const BUILT_IN_IDS = new Set(BUILT_IN_LAYOUTS.map((l) => l.id));

const layoutsFile = () => path.join(app.getPath('userData'), 'layouts.json');

// Same rules as settings.json: missing is normal, an unparseable file is
// moved aside once (so the next save starts clean instead of overwriting
// it), and any other read error throws so save() never builds on {}.
function read() {
  let raw;
  try {
    raw = fs.readFileSync(layoutsFile(), 'utf-8');
  } catch (err) {
    if (err.code === 'ENOENT') return {};
    throw err;
  }
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) return parsed;
  } catch {}

  const corruptCopy = corruptCopyPath(layoutsFile());
  try {
    fs.renameSync(layoutsFile(), corruptCopy);
  } catch (err) {
    throw new Error(`layouts.json is unreadable and could not be moved aside: ${err.message}`);
  }
  console.error(`layouts.json is unreadable; moved it to ${corruptCopy}`);
  return {};
}

// Built-ins first, then the user's valid layouts. Unknown keys are kept so
// a save from this version doesn't drop data a newer version wrote.
function normalize(data) {
  const custom = (Array.isArray(data.layouts) ? data.layouts : []).filter((l) => {
    if (l && BUILT_IN_IDS.has(l.id)) return false;
    const problem = zones.validateLayout(l);
    if (problem) console.error(`Skipping a layout in layouts.json: ${problem}`);
    return !problem;
  });
  const gap = Number.isInteger(data.gap) && data.gap >= 0 && data.gap <= MAX_GAP ? data.gap : DEFAULT_GAP;
  return { ...data, version: 1, gap, layouts: [...structuredClone(BUILT_IN_LAYOUTS), ...custom] };
}

function load() {
  try {
    return normalize(read());
  } catch {
    return normalize({}); // locked right now - use defaults for this read only
  }
}

function save(data) {
  const normalized = normalize(data);
  const toWrite = { ...normalized, layouts: normalized.layouts.filter((l) => !BUILT_IN_IDS.has(l.id)) };
  writeFileAtomic(layoutsFile(), JSON.stringify(toWrite, null, 2));
  return normalized;
}

module.exports = { load, save, BUILT_IN_LAYOUTS, DEFAULT_GAP };
