// The stock apps a fresh install starts with (File Explorer, then Recycle
// Bin, the order Explorer's own sidebar uses), and the repair that puts them
// back the way this version ships them. Kept free of Electron and icon work
// so it can be tested; main.js fetches the icons.
const layout = require('./layout');

// Recycle Bin isn't a real file (it's a virtual shell folder), so it's a
// custom command opening the special shell namespace path, and the renderer
// draws its icon (`special`) rather than one extracted from a target.
const EXPLORER_PATH = 'C:\\Windows\\explorer.exe';
const RECYCLE_BIN_CLSID = '::{645FF040-5081-101B-9F08-00AA002F954E}';

// `key` is how a default is found again in apps.json: not by name, which
// the user can change.
const DEFAULTS = [
  {
    key: 'explorer',
    name: 'File Explorer',
    type: 'path',
    target: EXPLORER_PATH,
    command: '',
    special: null,
  },
  {
    key: 'recycleBin',
    name: 'Recycle Bin',
    type: 'command',
    target: '',
    command: `explorer.exe ${RECYCLE_BIN_CLSID}`,
    // Windows won't hand over its real recycle-bin icon through any path
    // form app.getFileIcon understands (tried the CLSID, shell: URI, the
    // registry-documented imageres.dll,-55 resource, and the raw $Recycle.Bin
    // folder - all just return a generic file/folder icon). The renderer
    // draws a proper trash-can glyph for any item flagged like this instead.
    special: 'recycleBin',
  },
];

// Entries carry `stockKey` from this version on. Older ones are recognised
// by what they launch (Explorer's path, the Recycle Bin's flag), and failing
// that by the stock name, so a broken target can still be repaired.
function matchesDefault(entry, def) {
  if (!entry || entry.kind !== 'app') return false;
  if (entry.stockKey) return entry.stockKey === def.key;
  if (def.special) return entry.special === def.special;
  if (entry.special) return false;
  const target = typeof entry.target === 'string' ? entry.target.toLowerCase() : '';
  return target === def.target.toLowerCase() || String(entry.name || '').toLowerCase() === def.name.toLowerCase();
}

// Fresh entries for a first run, laid out explicitly (Explorer at 0,0, the
// Recycle Bin beside it) rather than left to array or createdAt order.
function buildEntries(newId, now = Date.now()) {
  return DEFAULTS.map((def, i) => ({
    id: newId(),
    kind: 'app',
    name: def.name,
    type: def.type,
    target: def.target,
    args: '',
    cwd: '',
    interpreter: '',
    command: def.command,
    parentId: null,
    pinned: false,
    createdAt: now + i, // Explorer is the older one
    lastLaunched: null,
    launchCount: 0,
    color: null,
    iconPath: null,
    iconIsCustom: false,
    gridX: i,
    gridY: 0,
    stockKey: def.key,
    ...(def.special ? { special: def.special } : {}),
  }));
}

// Repairs the stock apps in `apps` (mutated in place): each one found by
// its key gets this version's type, target and command back (its name,
// island, position, pin and colour are the user's and stay), and any that's
// missing is added at Home on the nearest free cell to where a first run
// puts it. Returns { repaired, added }: the entries touched, for the caller
// to refresh icons on.
function repair(apps, newId, now = Date.now()) {
  const fresh = buildEntries(newId, now);
  const repaired = [];
  const added = [];
  DEFAULTS.forEach((def, i) => {
    const existing = apps.filter((a) => matchesDefault(a, def));
    if (!existing.length) {
      const entry = fresh[i];
      layout.placeFree(apps, entry, entry.gridX, entry.gridY);
      apps.push(entry);
      added.push(entry);
      return;
    }
    for (const entry of existing) {
      entry.stockKey = def.key;
      entry.type = def.type;
      entry.target = def.target;
      entry.command = def.command;
      if (def.special) entry.special = def.special;
      else delete entry.special;
      repaired.push(entry);
    }
  });
  return { repaired, added };
}

module.exports = { DEFAULTS, EXPLORER_PATH, RECYCLE_BIN_CLSID, matchesDefault, buildEntries, repair };
