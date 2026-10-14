const test = require('node:test');
const assert = require('node:assert');
const scenes = require('../lib/scenes');

const WORK = { x: 0, y: 40, width: 2000, height: 1000 };
const display = (id, x = 0) => ({ id, bounds: { x, y: 0, width: 2000, height: 1040 }, workArea: { x, y: 40, width: 2000, height: 1000 } });
const entry = (exe, title, rect = { x: 0, y: 0, w: 0.5, h: 1 }) => ({
  exe,
  title,
  display: { id: 1, bounds: { x: 0, y: 0, width: 2000, height: 1040 } },
  rect,
  maximized: false,
});
const scene = (windows = [entry('C:\\Apps\\Code.exe', 'main.js - Launchpad')]) => ({ id: 's1', name: 'Coding', islandId: null, savedAt: 1, windows });

test('validateScene accepts a saved scene and names what is wrong with a broken one', () => {
  assert.strictEqual(scenes.validateScene(scene()), null);
  assert.strictEqual(scenes.validateScene({ ...scene(), islandId: 'island-1' }), null);
  assert.match(scenes.validateScene({ ...scene(), id: '' }), /id/);
  assert.match(scenes.validateScene({ ...scene(), name: '   ' }), /name/);
  assert.match(scenes.validateScene({ ...scene(), islandId: 7 }), /island/);
  assert.match(scenes.validateScene(scene([])), /no windows/);
  assert.match(scenes.validateScene(scene([{ ...entry('a.exe', 't'), exe: '' }])), /app/);
  assert.match(scenes.validateScene(scene([{ ...entry('a.exe', 't'), rect: { x: 0.6, y: 0, w: 0.5, h: 1 } }])), /off its display/);
  assert.match(scenes.validateScene(scene([{ ...entry('a.exe', 't'), display: { id: 'x' } }])), /display/);
  assert.match(scenes.validateScene(scene([{ ...entry('a.exe', 't'), maximized: 'yes' }])), /maximized/);
});

test('normalizeScenes keeps valid scenes, first of each id', (t) => {
  t.mock.method(console, 'error', () => {});
  const kept = scenes.normalizeScenes([scene(), { ...scene(), name: 'Duplicate' }, { id: 'broken' }, null]);
  assert.deepStrictEqual(kept.map((s) => s.name), ['Coding']);
  assert.deepStrictEqual(scenes.normalizeScenes(undefined), []);
});

test('toFractions and fromFractions round-trip a window, and scale to another work area', () => {
  const frame = { x: 500, y: 290, width: 1000, height: 500 };
  const rect = scenes.toFractions(frame, WORK);
  assert.deepStrictEqual(rect, { x: 0.25, y: 0.25, w: 0.5, h: 0.5 });
  assert.deepStrictEqual(scenes.fromFractions(rect, WORK), frame);
  assert.deepStrictEqual(scenes.fromFractions(rect, { x: -1920, y: 0, width: 1920, height: 1080 }), {
    x: -1440,
    y: 270,
    width: 960,
    height: 540,
  });
});

test('toFractions cuts off the part of a window outside the work area', () => {
  assert.deepStrictEqual(scenes.toFractions({ x: -100, y: 0, width: 600, height: 2000 }, WORK), { x: 0, y: 0, w: 0.25, h: 1 });
  // Entirely off to the right: a sliver at the right edge, still valid.
  const off = scenes.toFractions({ x: 2500, y: 100, width: 300, height: 300 }, WORK);
  assert.ok(off.x + off.w <= 1 && off.w > 0);
});

test('pickDisplay prefers the same display, then one with the same bounds, then the primary', () => {
  const a = display(1);
  const b = display(2, 2000);
  const primary = a;
  assert.strictEqual(scenes.pickDisplay({ id: 2, bounds: b.bounds }, [a, b], primary), b);
  assert.strictEqual(scenes.pickDisplay({ id: 99, bounds: b.bounds }, [a, b], primary), b);
  assert.strictEqual(scenes.pickDisplay({ id: 99, bounds: { x: 9, y: 9, width: 9, height: 9 } }, [a, b], primary), primary);
});

test('matchWindows tells windows of one app apart by title, then takes the rest in z-order', () => {
  const entries = [entry('C:\\FF\\firefox.exe', 'Docs'), entry('C:\\FF\\firefox.exe', 'Mail'), entry('C:\\Apps\\Spotify.exe', 'Spotify')];
  const windows = [
    { hwnd: 1, exe: 'c:\\ff\\FIREFOX.EXE', title: 'News' }, // top of the z-order
    { hwnd: 2, exe: 'C:\\FF\\firefox.exe', title: 'Mail' },
    { hwnd: 3, exe: 'C:\\Apps\\Code.exe', title: 'Code' },
  ];
  const { matched, missing } = scenes.matchWindows(entries, windows);
  assert.deepStrictEqual(
    matched.map((m) => [m.entry.title, m.window.hwnd]),
    [
      ['Docs', 1], // no title match, so the top Firefox window left
      ['Mail', 2], // same title
    ]
  );
  assert.deepStrictEqual(missing.map((e) => e.title), ['Spotify']);
});

test('matchWindows uses each open window once', () => {
  const entries = [entry('a.exe', 'One'), entry('a.exe', 'Two')];
  const { matched, missing } = scenes.matchWindows(entries, [{ hwnd: 5, exe: 'a.exe', title: 'Two' }]);
  assert.deepStrictEqual(matched.map((m) => m.entry.title), ['Two']);
  assert.deepStrictEqual(missing.map((e) => e.title), ['One']);
});

test('nextSceneName takes the first free number', () => {
  assert.strictEqual(scenes.nextSceneName([]), 'Scene 1');
  assert.strictEqual(scenes.nextSceneName([{ name: 'Scene 1' }, { name: 'Scene 3' }]), 'Scene 2');
});

test("appExe is an app's .exe target; appName prefers the user's name for it", () => {
  const code = { kind: 'app', type: 'path', target: 'C:\\Apps\\Code.exe', name: 'VS Code' };
  assert.strictEqual(scenes.appExe(code), 'C:\\Apps\\Code.exe');
  assert.strictEqual(scenes.appExe({ ...code, type: 'url', target: 'https://example.com' }), null);
  assert.strictEqual(scenes.appExe({ ...code, target: 'C:\\Projects' }), null);
  assert.strictEqual(scenes.appExe({ kind: 'island' }), null);
  assert.strictEqual(scenes.appName('c:\\apps\\code.exe', [code]), 'VS Code');
  assert.strictEqual(scenes.appName('C:\\Other\\Spotify.exe', [code]), 'Spotify');
});
