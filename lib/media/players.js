// Media's player and emulator profiles (profiles.json): which program
// opens a video or ROM, with which arguments. Args are an array of strings,
// never one command line, so a file name is passed as exactly one argument
// with no quoting or splitting - see buildLaunch().
const fs = require('fs');
const path = require('path');
const { app } = require('electron');
const { writeFileAtomic, corruptCopyPath } = require('../safe-file');

const profilesFile = () => path.join(app.getPath('userData'), 'profiles.json');

const KINDS = ['player', 'emulator'];
const ID_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;
// Platform ids are short slugs (snes, psx, n64); 'videos' is the key the
// video library uses for its default player.
const PLATFORM_RE = /^[a-z0-9][a-z0-9-]{0,31}$/;
const VIDEOS = 'videos';
const PLACEHOLDER_RE = /\{(file|dir|name)\}/g;

// Starting points for the profile editor. `exe` is filled in by the user or
// by detectInstalled(); `candidates` are the usual install locations.
const PRESETS = [
  {
    id: 'mpv',
    name: 'mpv',
    kind: 'player',
    args: ['--fs', '{file}'],
    platforms: [VIDEOS],
    candidates: ['C:\\Program Files\\mpv\\mpv.exe', 'C:\\Program Files (x86)\\mpv\\mpv.exe'],
  },
  {
    id: 'vlc',
    name: 'VLC',
    kind: 'player',
    args: ['--fullscreen', '--play-and-exit', '{file}'],
    platforms: [VIDEOS],
    candidates: ['C:\\Program Files\\VideoLAN\\VLC\\vlc.exe', 'C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe'],
  },
  {
    id: 'retroarch-snes',
    name: 'RetroArch (Snes9x)',
    kind: 'emulator',
    // The core path is relative to RetroArch's folder, which buildLaunch()
    // uses as the working directory.
    args: ['-L', 'cores\\snes9x_libretro.dll', '-f', '{file}'],
    platforms: ['snes'],
    candidates: ['C:\\RetroArch-Win64\\retroarch.exe', 'C:\\Program Files\\RetroArch\\retroarch.exe'],
  },
];

// Returns a list of problems with a profile; empty means it can be saved.
function validateProfile(p) {
  const errors = [];
  if (!p || typeof p !== 'object') return ['Profile is missing'];
  if (typeof p.id !== 'string' || !ID_RE.test(p.id)) errors.push('Id must be lower-case letters, digits and dashes');
  if (typeof p.name !== 'string' || !p.name.trim()) errors.push('Name is required');
  if (!KINDS.includes(p.kind)) errors.push('Kind must be player or emulator');
  if (typeof p.exe !== 'string' || !path.win32.isAbsolute(p.exe)) {
    errors.push('Program must be a full path');
  } else if (/\.(bat|cmd)$/i.test(p.exe)) {
    // Node only runs these through cmd.exe, which would interpret & and %
    // in file names. Scripted setups belong in a normal Launchpad item.
    errors.push('Batch files (.bat, .cmd) cannot be used as a player or emulator');
  } else if (!/\.exe$/i.test(p.exe)) {
    errors.push('Program must be an .exe');
  }
  if (!Array.isArray(p.args) || !p.args.every((a) => typeof a === 'string')) errors.push('Arguments must be a list of text');
  if (!Array.isArray(p.platforms) || !p.platforms.every((x) => typeof x === 'string' && PLATFORM_RE.test(x))) {
    errors.push('Platforms must be a list of platform ids');
  }
  return errors;
}

// The program, arguments and working directory to spawn for one file.
// Placeholders are replaced inside each argument with a function, so a
// file name containing `$&` or `$1` is inserted literally.
function buildLaunch(profile, file) {
  const errors = validateProfile(profile);
  if (errors.length) throw new Error(errors[0]);
  if (typeof file !== 'string' || !file) throw new Error('No file to open');
  const values = {
    file,
    dir: path.win32.dirname(file),
    name: path.win32.basename(file, path.win32.extname(file)),
  };
  return {
    exe: profile.exe,
    args: profile.args.map((a) => a.replace(PLACEHOLDER_RE, (_m, key) => values[key])),
    cwd: path.win32.dirname(profile.exe),
  };
}

// A preset as a saveable profile, with the program the user picked.
function fromPreset(presetId, exe) {
  const preset = PRESETS.find((p) => p.id === presetId);
  if (!preset) throw new Error(`Unknown preset: ${presetId}`);
  const { candidates, ...profile } = preset;
  return { ...profile, args: [...profile.args], platforms: [...profile.platforms], exe };
}

// Presets whose program is installed in one of its usual places.
function detectInstalled(exists = fs.existsSync) {
  const found = [];
  for (const preset of PRESETS) {
    const exe = preset.candidates.find((c) => exists(c));
    if (exe) found.push(fromPreset(preset.id, exe));
  }
  return found;
}

function emptyData() {
  return { profiles: [], defaults: {} };
}

// Keeps only valid profiles and defaults that point at one of them.
function sanitize(data) {
  const out = emptyData();
  if (!data || typeof data !== 'object' || Array.isArray(data)) return out;
  const seen = new Set();
  for (const p of Array.isArray(data.profiles) ? data.profiles : []) {
    if (validateProfile(p).length || seen.has(p.id)) continue;
    seen.add(p.id);
    out.profiles.push({ id: p.id, name: p.name, kind: p.kind, exe: p.exe, args: p.args, platforms: p.platforms });
  }
  const defaults = data.defaults && typeof data.defaults === 'object' ? data.defaults : {};
  for (const [platform, id] of Object.entries(defaults)) {
    if (PLATFORM_RE.test(platform) && seen.has(id)) out.defaults[platform] = id;
  }
  return out;
}

// Like settings.json: a missing file is empty, and an unreadable one is
// moved aside once so the next save starts fresh instead of failing forever.
// Locked files throw, so a save never replaces data it couldn't read.
function load() {
  let raw;
  try {
    raw = fs.readFileSync(profilesFile(), 'utf-8');
  } catch (err) {
    if (err.code === 'ENOENT') return emptyData();
    throw err;
  }
  try {
    return sanitize(JSON.parse(raw));
  } catch {}
  const corruptCopy = corruptCopyPath(profilesFile());
  fs.renameSync(profilesFile(), corruptCopy);
  console.error(`profiles.json is unreadable; moved it to ${corruptCopy}`);
  return emptyData();
}

function save(data) {
  const clean = sanitize(data);
  fs.mkdirSync(path.dirname(profilesFile()), { recursive: true });
  writeFileAtomic(profilesFile(), JSON.stringify(clean, null, 2));
  return clean;
}

// The profile for a library entry: its own, else its platform's default
// (videos use the 'videos' default). Null when none is set up.
function profileFor(entry, data) {
  const byId = (id) => data.profiles.find((p) => p.id === id) || null;
  if (entry.profile) return byId(entry.profile);
  const key = entry.library === 'videos' ? VIDEOS : entry.platform;
  return key && data.defaults[key] ? byId(data.defaults[key]) : null;
}

module.exports = {
  load,
  save,
  sanitize,
  validateProfile,
  buildLaunch,
  fromPreset,
  detectInstalled,
  profileFor,
  profilesFile,
  PRESETS,
  KINDS,
  VIDEOS,
};
