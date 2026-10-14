// Window layouts for the organizer, kept in <userData>/layouts.json next to
// apps.json. The built-in layouts always exist and are never written to the
// file, so a later version can adjust them; the file only holds the user's
// own layouts (made in the Layouts editor), which layouts are hidden,
// organizer preferences such as the gap, and the user's scenes (saved
// window arrangements, see lib/scenes.js). A missing file just means
// "built-ins and defaults".
const fs = require('fs');
const path = require('path');
const { app } = require('electron');
const zones = require('./zones');
const scenes = require('./scenes');
const { writeFileAtomic, corruptCopyPath } = require('./safe-file');

const DEFAULT_GAP = 8;
const MAX_GAP = 64;
const MAX_NAME_LENGTH = 40;

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
  const seen = new Set(BUILT_IN_IDS);
  const custom = (Array.isArray(data.layouts) ? data.layouts : []).filter((l) => {
    if (l && seen.has(l.id)) return false; // a built-in's id, or one already used
    const problem = zones.validateLayout(l);
    if (problem) console.error(`Skipping a layout in layouts.json: ${problem}`);
    else seen.add(l.id);
    return !problem;
  });
  const gap = Number.isInteger(data.gap) && data.gap >= 0 && data.gap <= MAX_GAP ? data.gap : DEFAULT_GAP;
  const hidden = Array.isArray(data.hidden) ? [...new Set(data.hidden.filter((id) => seen.has(id)))] : [];
  return {
    ...data,
    version: 1,
    gap,
    hidden,
    layouts: [...structuredClone(BUILT_IN_LAYOUTS), ...custom],
    scenes: scenes.normalizeScenes(data.scenes),
  };
}

function load() {
  try {
    return normalize(read());
  } catch {
    return normalize({}); // locked right now - use defaults for this read only
  }
}

// What goes in the file: everything but the built-ins.
const fileContents = (normalized) => ({ ...normalized, layouts: normalized.layouts.filter((l) => !BUILT_IN_IDS.has(l.id)) });

function save(data) {
  const normalized = normalize(data);
  writeFileAtomic(layoutsFile(), JSON.stringify(fileContents(normalized), null, 2));
  return normalized;
}

// The layouts the panel, the drag strip and the taskbar's "Snap to" menu
// offer: all but the hidden ones. Should every one be hidden (a hand-edited
// file), all of them, so there's always something to snap to.
function visible(data) {
  const hidden = new Set(data.hidden);
  const shown = data.layouts.filter((l) => !hidden.has(l.id));
  return shown.length ? shown : data.layouts;
}

// Every layout, marked for the editor, plus the gap.
function library(data) {
  const hidden = new Set(data.hidden);
  return {
    layouts: data.layouts.map((l) => ({ ...l, builtIn: BUILT_IN_IDS.has(l.id), hidden: hidden.has(l.id) })),
    gap: data.gap,
  };
}

// For the editor's changes: unlike load(), a file that can't be read right
// now throws instead of reading as empty, so saving one layout can never
// wipe out the others.
function update(change) {
  const data = normalize(read());
  change(data);
  return save(data);
}

const cleanName = (name) => (typeof name === 'string' ? name.trim().slice(0, MAX_NAME_LENGTH) : '');

// Adds the layout, or replaces the one with its id. Throws a message ready
// for the user if it isn't one that can be saved.
function saveLayout(layout) {
  if (!layout || typeof layout !== 'object') throw new Error('That layout is missing.');
  if (BUILT_IN_IDS.has(layout.id)) throw new Error("Built-in layouts can't be changed. Duplicate it to make your own.");
  const clean = {
    id: layout.id,
    name: cleanName(layout.name),
    cols: layout.cols,
    rows: layout.rows,
    zones: (Array.isArray(layout.zones) ? layout.zones : []).map((z) => ({
      id: z && z.id,
      name: cleanName(z && z.name),
      x: z && z.x,
      y: z && z.y,
      w: z && z.w,
      h: z && z.h,
    })),
  };
  const problem = zones.validateLayout(clean);
  if (problem) throw new Error(`That layout can't be saved: ${problem}.`);
  return update((data) => {
    const i = data.layouts.findIndex((l) => l.id === clean.id);
    if (i === -1) data.layouts.push(clean);
    else data.layouts[i] = clean;
  });
}

function removeLayout(id) {
  if (BUILT_IN_IDS.has(id)) throw new Error("Built-in layouts can't be deleted, only hidden.");
  return update((data) => {
    data.layouts = data.layouts.filter((l) => l.id !== id);
  });
}

function setHidden(id, hide) {
  return update((data) => {
    if (!data.layouts.some((l) => l.id === id)) throw new Error('That layout no longer exists.');
    const others = data.hidden.filter((h) => h !== id);
    if (hide && others.length + 1 >= data.layouts.length) throw new Error('At least one layout has to stay shown.');
    data.hidden = hide ? [...others, id] : others;
  });
}

function setGap(gap) {
  if (!Number.isInteger(gap) || gap < 0 || gap > MAX_GAP) throw new Error(`The gap has to be a whole number from 0 to ${MAX_GAP}.`);
  return update((data) => {
    data.gap = gap;
  });
}

// ---------- scenes ----------

// Adds the scene, or replaces the one with its id. scene is as captured by
// lib/scene-runner.js; it's checked here like a scene read from the file.
function saveScene(scene) {
  const clean = { ...scene, name: scenes.cleanName(scene && scene.name) };
  const problem = scenes.validateScene(clean);
  if (problem) throw new Error(`That scene can't be saved: ${problem}.`);
  return update((data) => {
    const i = data.scenes.findIndex((s) => s.id === clean.id);
    if (i === -1) data.scenes.push(clean);
    else data.scenes[i] = clean;
  });
}

function renameScene(id, name) {
  const clean = scenes.cleanName(name);
  if (!clean) throw new Error('A scene needs a name.');
  return update((data) => {
    const scene = data.scenes.find((s) => s.id === id);
    if (!scene) throw new Error('That scene no longer exists.');
    scene.name = clean;
  });
}

function removeScene(id) {
  return update((data) => {
    data.scenes = data.scenes.filter((s) => s.id !== id);
  });
}

// For backups: what the file holds (the user's layouts, which ones are
// hidden, the gap, the scenes). Throws when the file can't be read, so an
// export never quietly leaves them out.
function exportData() {
  return fileContents(normalize(read()));
}

// Replaces the file with a backup's layouts (anything invalid is dropped).
function importData(data) {
  return save(data && typeof data === 'object' && !Array.isArray(data) ? data : {});
}

module.exports = {
  load,
  save,
  visible,
  library,
  saveLayout,
  removeLayout,
  setHidden,
  setGap,
  saveScene,
  renameScene,
  removeScene,
  exportData,
  importData,
  BUILT_IN_LAYOUTS,
  DEFAULT_GAP,
};
