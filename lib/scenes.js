// Scenes: saved window arrangements for the organizer. A scene remembers,
// for each window that was open when it was saved, the app (by exe path),
// the window's title (to tell two windows of one app apart), the display it
// was on, and where on that display's work area it sat, as fractions of the
// work area, so it comes back in proportion on a different resolution.
// Maximized windows are remembered as maximized. Scenes live in
// layouts.json (lib/layouts.js); a scene saved from an island has that
// island's id, and "Open as scene" on the island applies it.
//
// Everything here is pure, so it can be tested without Electron or the
// window helper; lib/scene-runner.js does the saving and applying.

const MAX_NAME_LENGTH = 40;
const MAX_WINDOWS = 64;
const MAX_TEXT = 1024;

const isNumber = (v) => typeof v === 'number' && Number.isFinite(v);
const isRect = (r, keys) => !!r && typeof r === 'object' && keys.every((k) => isNumber(r[k]));
const cleanName = (name) => (typeof name === 'string' ? name.trim().slice(0, MAX_NAME_LENGTH) : '');

// Why a scene can't be used, or null if it can.
function validateScene(s) {
  if (!s || typeof s !== 'object') return 'it is not a scene';
  if (typeof s.id !== 'string' || !s.id || s.id.length > 64) return 'it has no id';
  if (!cleanName(s.name)) return 'it has no name';
  if (s.islandId !== null && typeof s.islandId !== 'string') return 'its island is not an id';
  if (!Array.isArray(s.windows) || s.windows.length === 0) return 'it has no windows';
  if (s.windows.length > MAX_WINDOWS) return `it has more than ${MAX_WINDOWS} windows`;
  for (const w of s.windows) {
    if (!w || typeof w.exe !== 'string' || !w.exe || w.exe.length > MAX_TEXT) return 'a window has no app';
    if (typeof w.title !== 'string' || w.title.length > MAX_TEXT) return 'a window has no title';
    if (!w.display || !isNumber(w.display.id) || !isRect(w.display.bounds, ['x', 'y', 'width', 'height'])) {
      return 'a window has no display';
    }
    if (!isRect(w.rect, ['x', 'y', 'w', 'h'])) return 'a window has no position';
    const { x, y, w: width, h } = w.rect;
    if (x < 0 || y < 0 || width <= 0 || h <= 0 || x + width > 1.0001 || y + h > 1.0001) return 'a window is off its display';
    if (typeof w.maximized !== 'boolean') return 'a window has no maximized flag';
  }
  return null;
}

// The scenes worth keeping from a file: valid ones, first of each id.
function normalizeScenes(list) {
  const seen = new Set();
  return (Array.isArray(list) ? list : []).filter((s) => {
    const problem = validateScene(s);
    if (problem) console.error(`Skipping a scene in layouts.json: ${problem}`);
    if (problem || seen.has(s.id)) return false;
    seen.add(s.id);
    return true;
  });
}

const round4 = (n) => Math.round(n * 10000) / 10000;

// A window's frame (DIP) as fractions of a work area. The part of the window
// outside the work area is cut off; a window entirely outside it ends up as
// a thin sliver at the nearest edge rather than being lost.
function toFractions(frame, workArea) {
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  const left = clamp(frame.x, workArea.x, workArea.x + workArea.width - 1);
  const top = clamp(frame.y, workArea.y, workArea.y + workArea.height - 1);
  const right = clamp(frame.x + frame.width, left + 1, workArea.x + workArea.width);
  const bottom = clamp(frame.y + frame.height, top + 1, workArea.y + workArea.height);
  return {
    x: round4((left - workArea.x) / workArea.width),
    y: round4((top - workArea.y) / workArea.height),
    w: round4((right - left) / workArea.width),
    h: round4((bottom - top) / workArea.height),
  };
}

// Back to a DIP rect on a (possibly different) work area.
function fromFractions(rect, workArea) {
  const left = Math.round(workArea.x + rect.x * workArea.width);
  const top = Math.round(workArea.y + rect.y * workArea.height);
  const right = Math.round(workArea.x + (rect.x + rect.w) * workArea.width);
  const bottom = Math.round(workArea.y + (rect.y + rect.h) * workArea.height);
  return { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) };
}

const sameBounds = (a, b) => a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;

// The display a saved window goes back to: the same one if it's still
// there, else one in the same place with the same size, else the primary.
function pickDisplay(saved, displays, primary) {
  return (
    displays.find((d) => d.id === saved.id) ||
    displays.find((d) => sameBounds(d.bounds, saved.bounds)) ||
    primary
  );
}

const exeKey = (exe) => (exe || '').toLowerCase();

// Pairs a scene's windows with open windows ({ hwnd, exe, title }, in
// z-order, top first). Windows of the same app are told apart by title
// first; whatever is left of that app goes in z-order. Each open window is
// used once. Returns { matched: [{ entry, window }], missing: [entry] }.
function matchWindows(entries, windows) {
  const free = windows.slice();
  const pairs = new Array(entries.length).fill(null);
  const take = (i, test) => {
    const j = free.findIndex((w) => exeKey(w.exe) === exeKey(entries[i].exe) && test(w));
    if (j !== -1) pairs[i] = free.splice(j, 1)[0];
  };
  entries.forEach((e, i) => take(i, (w) => w.title === e.title));
  entries.forEach((e, i) => {
    if (!pairs[i]) take(i, () => true);
  });
  return {
    matched: entries.map((entry, i) => (pairs[i] ? { entry, window: pairs[i] } : null)).filter(Boolean),
    missing: entries.filter((_e, i) => !pairs[i]),
  };
}

// "Scene 1", "Scene 2"…: the first number no scene's name uses yet.
function nextSceneName(scenes) {
  const taken = new Set(scenes.map((s) => s.name));
  for (let n = 1; ; n++) {
    if (!taken.has(`Scene ${n}`)) return `Scene ${n}`;
  }
}

// The exe an island's app runs as, for matching it to windows: its target
// when that's an .exe (shortcuts are resolved to their target when added).
// null for websites, scripts, commands and folders, which can't be told
// apart by their windows.
function appExe(app) {
  return app && app.kind === 'app' && app.type === 'path' && typeof app.target === 'string' && /\.exe$/i.test(app.target)
    ? app.target
    : null;
}

// A name for an app in a message: Launchpad's own name for it when it's
// one of the user's apps, else the exe's file name without ".exe",
// capitalized ("firefox.exe" -> "Firefox").
function appName(exe, apps) {
  const app = (apps || []).find((a) => appExe(a) && exeKey(appExe(a)) === exeKey(exe));
  if (app) return app.name;
  const file = String(exe).split(/[\\/]/).pop().replace(/\.exe$/i, '') || String(exe);
  return file.charAt(0).toUpperCase() + file.slice(1);
}

module.exports = {
  MAX_NAME_LENGTH,
  cleanName,
  validateScene,
  normalizeScenes,
  toFractions,
  fromFractions,
  pickDisplay,
  matchWindows,
  nextSceneName,
  appExe,
  appName,
  exeKey,
};
