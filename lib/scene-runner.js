// Saves and applies scenes (lib/scenes.js) against the real windows, through
// the window helper. As in lib/organizer.js, positions are worked out in DIP
// on the Electron side and converted to physical pixels just before the
// helper moves a window, so mixed-DPI monitors line up.
const crypto = require('crypto');
const { screen } = require('electron');
const layouts = require('./layouts');
const scenes = require('./scenes');
const windowmgr = require('./windowmgr');

// Opening an island as a scene: how long to wait for the apps it launched
// to show a window, how often to look, and how long to let a new window
// settle (apps often move themselves to where they last were just after
// showing) before placing it, and once more after that.
const LAUNCH_TIMEOUT_MS = 20000;
const LAUNCH_POLL_MS = 500;
const SETTLE_MS = 1000;
const SECOND_PASS_MS = 1500;

const NOTHING_OPEN = 'There are no app windows open to save. Open and arrange some first.';
const ISLAND_NOTHING_OPEN = "None of this island's apps has a window open. Open them, arrange them, then save.";

let busy = false;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const physToDip = ([l, t, r, b]) => screen.screenToDipRect(null, { x: l, y: t, width: r - l, height: b - t });

async function openWindows() {
  const reply = await windowmgr.listWindows();
  if (!reply.ok) throw new Error("Launchpad couldn't list the open windows.");
  return reply.windows;
}

// What a scene remembers about an open window. Minimized ones aren't part
// of the arrangement on screen, so they're left out (null).
function captureWindow(win) {
  if (win.minimized) return null;
  const frame = physToDip(win.frame);
  const display = screen.getDisplayMatching(frame);
  return {
    exe: win.exe,
    title: win.title.slice(0, 1024),
    display: { id: display.id, bounds: display.bounds },
    rect: win.maximized ? { x: 0, y: 0, w: 1, h: 1 } : scenes.toFractions(frame, display.workArea),
    maximized: !!win.maximized,
  };
}

// Saves where the open windows are now. With id, over that scene (keeping
// its name and island); with islandId, as that island's scene, holding
// only the windows of the apps in exes (and replacing the island's last
// one). Otherwise as a new scene, named name or "Scene n". Resolves to the
// saved scene; throws a message ready for the user.
async function save({ id = null, name = null, islandId = null, exes = null } = {}) {
  const wanted = exes && new Set([...exes].map(scenes.exeKey));
  const captured = (await openWindows())
    .filter((w) => !wanted || wanted.has(scenes.exeKey(w.exe)))
    .map(captureWindow)
    .filter(Boolean)
    .slice(0, 64);
  if (!captured.length) throw new Error(islandId ? ISLAND_NOTHING_OPEN : NOTHING_OPEN);
  const all = layouts.load().scenes;
  const existing = id ? all.find((s) => s.id === id) : islandId ? all.find((s) => s.islandId === islandId) : null;
  if (id && !existing) throw new Error('That scene no longer exists.');
  const scene = {
    id: existing ? existing.id : crypto.randomUUID(),
    name: name || (existing && existing.name) || scenes.nextSceneName(all),
    islandId: islandId || (existing && existing.islandId) || null,
    savedAt: Date.now(),
    windows: captured,
  };
  layouts.saveScene(scene);
  return scene;
}

// Moves every open window the scene has a place for back there. Resolves to
// { placed, missing, failed }: how many windows moved, and the apps (named
// by nameFor(exe)) with no open window, or that Windows wouldn't let move.
async function placeScene(scene, nameFor) {
  const { matched, missing } = scenes.matchWindows(scene.windows, await openWindows());
  const displays = screen.getAllDisplays();
  const primary = screen.getPrimaryDisplay();
  let placed = 0;
  const failed = [];
  for (const { entry, window } of matched) {
    const display = scenes.pickDisplay(entry.display, displays, primary);
    const rect = screen.dipToScreenRect(null, scenes.fromFractions(entry.rect, display.workArea));
    let reply;
    try {
      reply = await windowmgr.place(window.hwnd, rect, entry.maximized);
    } catch {
      reply = { ok: false };
    }
    if (reply.ok) placed++;
    else failed.push(nameFor(entry.exe));
  }
  const unique = (names) => [...new Set(names)];
  return { placed, missing: unique(missing.map((e) => nameFor(e.exe))), failed: unique(failed) };
}

async function exclusive(fn) {
  if (busy) throw new Error('A scene is already being arranged. Try again in a moment.');
  busy = true;
  try {
    return await fn();
  } finally {
    busy = false;
  }
}

function sceneById(id) {
  const scene = layouts.load().scenes.find((s) => s.id === id);
  if (!scene) throw new Error('That scene no longer exists.');
  return scene;
}

// Resolves to { placed, missing, failed } (see placeScene).
function apply(id, nameFor) {
  return exclusive(() => placeScene(sceneById(id), nameFor));
}

// "Open as scene" on an island: launches each of the island's apps that the
// scene has a window for and that has none open now (launch(app) does the
// launching), waits for their windows, and applies the scene. Resolves to
// { launched, placed, missing, failed }.
function openIsland(islandId, apps, launch, nameFor) {
  return exclusive(async () => {
    const scene = layouts.load().scenes.find((s) => s.islandId === islandId);
    if (!scene) throw new Error("This island has no saved arrangement yet. Arrange its apps, then use \"Save arrangement for this island\".");
    const inScene = new Set(scene.windows.map((w) => scenes.exeKey(w.exe)));
    const running = new Set((await openWindows()).map((w) => scenes.exeKey(w.exe)));
    const toLaunch = new Map(); // exe key -> app, one launch per app
    for (const app of apps) {
      const key = scenes.exeKey(scenes.appExe(app));
      if (app.parentId === islandId && inScene.has(key) && !running.has(key) && !toLaunch.has(key)) toLaunch.set(key, app);
    }
    const launched = [];
    const notStarted = [];
    for (const [key, app] of toLaunch) {
      try {
        await launch(app);
        launched.push(key);
      } catch {
        notStarted.push(app.name);
      }
    }
    if (launched.length) {
      const waitingFor = new Set(launched);
      const deadline = Date.now() + LAUNCH_TIMEOUT_MS;
      while (Date.now() < deadline) {
        await sleep(LAUNCH_POLL_MS);
        const { missing } = scenes.matchWindows(scene.windows, await openWindows());
        if (!missing.some((e) => waitingFor.has(scenes.exeKey(e.exe)))) break;
      }
      await sleep(SETTLE_MS);
    }
    let result = await placeScene(scene, nameFor);
    if (launched.length) {
      await sleep(SECOND_PASS_MS);
      result = await placeScene(scene, nameFor);
    }
    // Apps that couldn't be started are named once, not again as "not open".
    const missing = result.missing.filter((name) => !notStarted.includes(name));
    return { ...result, missing, launched: launched.length, notStarted };
  });
}

// "Placed 3 windows. Not open: Spotify." for a panel message or a dialog.
function describe(result) {
  const parts = [];
  if (result.launched) parts.push(`Opened ${result.launched} app${result.launched === 1 ? '' : 's'}.`);
  parts.push(`Placed ${result.placed} window${result.placed === 1 ? '' : 's'}.`);
  if (result.notStarted && result.notStarted.length) parts.push(`Couldn't open: ${result.notStarted.join(', ')}.`);
  if (result.missing.length) parts.push(`Not open: ${result.missing.join(', ')}.`);
  if (result.failed.length) parts.push(`Couldn't move: ${result.failed.join(', ')}.`);
  return parts.join(' ');
}

module.exports = { save, apply, openIsland, describe };
