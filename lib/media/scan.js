// Finds games and videos in a library folder and merges them into the
// library (lib/media/library.js). The walk is the only part that touches
// the disk; detection, naming and merging are plain functions so they can
// be tested without a real ROM collection.
const fs = require('fs');
const path = require('path');
const library = require('./library');

// Folder aliases are compared after lower-casing and dropping everything
// but letters and digits, so "Nintendo 64", "nintendo-64" and "N64" match.
// Extensions shared by several systems (.iso, .chd, .cue, .bin) are left
// out: those platforms are only found by folder name or a user pick.
const PLATFORMS = [
  { id: 'nes', name: 'NES', aliases: ['nes', 'famicom', 'nintendoentertainmentsystem'], exts: ['.nes', '.fds'] },
  { id: 'snes', name: 'SNES', aliases: ['snes', 'superfamicom', 'sfc', 'supernintendo'], exts: ['.sfc', '.smc'] },
  { id: 'n64', name: 'N64', aliases: ['n64', 'nintendo64'], exts: ['.n64', '.z64', '.v64'] },
  { id: 'gb', name: 'Game Boy', aliases: ['gb', 'gameboy'], exts: ['.gb'] },
  { id: 'gbc', name: 'Game Boy Color', aliases: ['gbc', 'gameboycolor'], exts: ['.gbc'] },
  { id: 'gba', name: 'Game Boy Advance', aliases: ['gba', 'gameboyadvance'], exts: ['.gba'] },
  { id: 'nds', name: 'Nintendo DS', aliases: ['nds', 'ds', 'nintendods'], exts: ['.nds'] },
  { id: 'genesis', name: 'Genesis', aliases: ['genesis', 'megadrive', 'md'], exts: ['.md', '.gen', '.smd'] },
  { id: 'mastersystem', name: 'Master System', aliases: ['mastersystem', 'sms'], exts: ['.sms'] },
  { id: 'psx', name: 'PS1', aliases: ['psx', 'ps1', 'playstation', 'playstation1'], exts: [] },
  { id: 'ps2', name: 'PS2', aliases: ['ps2', 'playstation2'], exts: [] },
  { id: 'psp', name: 'PSP', aliases: ['psp'], exts: ['.cso'] },
  { id: 'gamecube', name: 'GameCube', aliases: ['gamecube', 'gc', 'ngc'], exts: ['.gcm', '.rvz'] },
  { id: 'wii', name: 'Wii', aliases: ['wii'], exts: ['.wbfs'] },
  { id: 'saturn', name: 'Saturn', aliases: ['saturn', 'segasaturn'], exts: [] },
  { id: 'dreamcast', name: 'Dreamcast', aliases: ['dreamcast', 'dc'], exts: ['.gdi', '.cdi'] },
  { id: 'arcade', name: 'Arcade', aliases: ['arcade', 'mame', 'fbneo'], exts: [] },
];

// Files any game platform may use, on top of its own extensions.
const SHARED_GAME_EXTS = ['.iso', '.chd', '.cue', '.bin', '.img', '.m3u', '.zip', '.7z', '.pbp'];
const VIDEO_EXTS = ['.mkv', '.mp4', '.avi', '.mov', '.m4v', '.webm', '.wmv', '.mpg', '.mpeg', '.ts'];
const MAX_DEPTH = 8;
const SKIP_DIRS = new Set(['$recycle.bin', 'system volume information', 'node_modules', 'downloaded_media', 'media']);

const keyOf = (s) => String(s).toLowerCase().replace(/[^a-z0-9]/g, '');
const samePath = (a, b) => a.toLowerCase() === b.toLowerCase();

function platformById(id) {
  return PLATFORMS.find((p) => p.id === id) || null;
}

// The platform for a file, from the nearest folder whose name is an alias,
// else from an extension only one platform uses. `dirs` are the folder
// names from the library root down to the file.
function detectPlatform(dirs, ext) {
  for (const dir of [...dirs].reverse()) {
    const key = keyOf(dir);
    const hit = PLATFORMS.find((p) => p.aliases.includes(key));
    if (hit) return hit.id;
  }
  const byExt = PLATFORMS.filter((p) => p.exts.includes(ext.toLowerCase()));
  return byExt.length === 1 ? byExt[0].id : null;
}

// "Chrono Trigger (USA) [!].sfc" -> Chrono Trigger. For videos a trailing
// year is kept apart: "Blade Runner 2049 (2017).mkv" -> name + year 2017.
function cleanName(file) {
  const raw = path.win32.basename(file, path.win32.extname(file));
  let base = raw;
  let year = null;
  const yearMatch = base.match(/[([]((?:19|20)\d{2})[)\]]/);
  if (yearMatch) year = Number(yearMatch[1]);
  base = base
    .replace(/\([^)]*\)|\[[^\]]*\]/g, ' ')
    .replace(/[_.]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  return { name: base || raw, year };
}

function isGameFile(ext, platform) {
  if (SHARED_GAME_EXTS.includes(ext)) return true;
  return PLATFORMS.some((p) => p.exts.includes(ext) && (!platform || p.id === platform));
}

// Disc images come as a playlist or cue sheet plus the track files it
// lists. Only the top file is a game: in a folder with an .m3u, the .cue,
// .bin and .img files are tracks; with a .cue, the .bin and .img are.
function dropTrackFiles(files) {
  const exts = new Set(files.map((f) => path.extname(f).toLowerCase()));
  let tracks = [];
  if (exts.has('.m3u')) tracks = ['.cue', '.bin', '.img', '.chd', '.iso'];
  else if (exts.has('.cue') || exts.has('.gdi')) tracks = ['.bin', '.img', '.raw'];
  return files.filter((f) => !tracks.includes(path.extname(f).toLowerCase()));
}

// Walks a library folder. Returns [{ target, name, year, platform, folder }],
// where folder is the subfolder relative to root ('' for the root itself).
// options: library ('games' | 'videos'), recursive (default true),
// platforms ({ '<relative folder>': platformId } user picks; '' = root).
async function scanFolder(root, { library: lib = 'games', recursive = true, platforms = {} } = {}) {
  const found = [];
  const overrideFor = (rel) => {
    for (let r = rel; ; r = path.dirname(r)) {
      const key = r === '.' ? '' : r;
      const hit = Object.keys(platforms).find((k) => samePath(k, key));
      if (hit !== undefined) return platforms[hit];
      if (!key) return null;
    }
  };

  async function walk(dir, dirs) {
    let dirents;
    try {
      dirents = await fs.promises.readdir(dir, { withFileTypes: true });
    } catch {
      return; // unreadable subfolder: skip it, keep the rest of the scan
    }
    const rel = dirs.join(path.sep);
    const files = dirents.filter((d) => d.isFile()).map((d) => d.name);
    const kept = lib === 'games' ? dropTrackFiles(files) : files;
    for (const name of kept) {
      const ext = path.extname(name).toLowerCase();
      if (lib === 'videos') {
        if (!VIDEO_EXTS.includes(ext)) continue;
        found.push({ target: path.join(dir, name), ...cleanName(name), platform: null, folder: rel });
        continue;
      }
      const platform = overrideFor(rel) || detectPlatform(dirs, ext);
      if (!isGameFile(ext, platform)) continue;
      const { name: clean } = cleanName(name);
      found.push({ target: path.join(dir, name), name: clean, year: null, platform, folder: rel });
    }
    if (!recursive || dirs.length >= MAX_DEPTH) return;
    for (const d of dirents) {
      // isDirectory() is false for junctions and symlinks, so loops can't happen.
      if (!d.isDirectory() || d.name.startsWith('.') || SKIP_DIRS.has(d.name.toLowerCase())) continue;
      await walk(path.join(dir, d.name), [...dirs, d.name]);
    }
  }

  await walk(root, []);
  return found;
}

// Counts per subfolder and platform, for the Add library folder screen.
function summarize(found) {
  const groups = new Map();
  for (const f of found) {
    const key = `${f.folder.toLowerCase()}\u0000${f.platform || ''}`;
    if (!groups.has(key)) groups.set(key, { folder: f.folder, platform: f.platform, count: 0 });
    groups.get(key).count++;
  }
  return [...groups.values()].sort((a, b) => a.folder.localeCompare(b.folder));
}

function isInside(file, root) {
  const rel = path.relative(root.toLowerCase(), file.toLowerCase());
  return !!rel && !rel.startsWith('..') && !path.isAbsolute(rel);
}

// Merges one scan of `root` into the library list. Files already there keep
// their id, art and play counts (matched by path, ignoring case); files no
// longer found are marked unavailable rather than removed; files that came
// back are available again. Entries outside root are never touched.
// Returns a new list plus counts.
function mergeScan(entries, found, { root, library: lib = 'games', now = Date.now() } = {}) {
  const byPath = new Map(found.map((f) => [f.target.toLowerCase(), f]));
  const counts = { added: 0, missing: 0, restored: 0 };
  const seen = new Set();

  const next = entries.map((e) => {
    if (e.library !== lib || !isInside(e.target, root)) return e;
    const key = e.target.toLowerCase();
    seen.add(key);
    const f = byPath.get(key);
    if (!f && e.available) {
      counts.missing++;
      return { ...e, available: false };
    }
    if (f && !e.available) {
      counts.restored++;
      return { ...e, available: true };
    }
    return e;
  });

  for (const f of found) {
    const key = f.target.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    counts.added++;
    next.push(
      library.normalize({
        id: library.newId(),
        library: lib,
        name: f.name,
        platform: f.platform,
        type: 'profile',
        target: f.target,
        createdAt: now,
        meta: f.year ? { year: f.year } : {},
      })
    );
  }
  return { entries: next, ...counts };
}

module.exports = {
  scanFolder,
  mergeScan,
  summarize,
  detectPlatform,
  cleanName,
  dropTrackFiles,
  platformById,
  PLATFORMS,
  VIDEO_EXTS,
  SHARED_GAME_EXTS,
};
