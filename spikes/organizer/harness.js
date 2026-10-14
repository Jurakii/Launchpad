// Phase 0 spike harness for the top bar window organizer. Throwaway code: it
// drives windowmgr-spike.ps1 from Electron to prove the risky Win32 pieces
// before any real organizer code is written. See README.md for results.
//
//   node_modules\electron\dist\electron.exe spikes\organizer\harness.js
//       automated checks only (helper, DPI conversion, AppBar, placement)
//   node_modules\electron\dist\electron.exe spikes\organizer\harness.js --watch 120
//       then for 120 s, snaps Notepad into the left half of whichever
//       monitor you drag it to, and records every drag
//
// Side effects while it runs: a 32px bar reserves the top of the primary
// monitor (maximized windows shift down, then back), a fullscreen window
// flashes for ~2 s, and a Notepad window opens and is closed at the end.
const { app, BrowserWindow, screen } = require('electron');
const { spawn } = require('child_process');
const readline = require('readline');
const fs = require('fs');
const path = require('path');

const WATCH_SECONDS = Number(argValue('--watch') || 0);
const APPBAR_CALLBACK = 0x8000 + 0x42; // WM_APP + 0x42
const PANEL_HEIGHT_DIP = 32;
const HTCAPTION = 2;
const OUT_FILE = path.join(__dirname, 'last-run.json');

const results = { startedAt: new Date().toISOString(), checks: [], appbarCallbacks: [], drags: [], foregroundLatencyMs: [] };
let helper = null;
let panel = null;
let notepadHwnd = null;

function argValue(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : null;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const toLTRB = (r) => [r.x, r.y, r.x + r.width, r.y + r.height];
const fromLTRB = ([l, t, r, b]) => ({ x: l, y: t, width: r - l, height: b - t });
const maxDiff = (a, b) => Math.max(...a.map((v, i) => Math.abs(v - b[i])));
const handleOf = (win) => Number(win.getNativeWindowHandle().readBigUInt64LE(0));
const leftHalf = (wa) => ({ x: wa.x, y: wa.y, width: Math.floor(wa.width / 2), height: wa.height });

function check(name, pass, detail) {
  results.checks.push({ name, pass, detail });
  const tag = pass === null ? 'INFO' : pass ? 'PASS' : 'FAIL';
  console.log(`${tag}  ${name}${detail ? '\n      ' + JSON.stringify(detail) : ''}`);
}

function startHelper() {
  const t0 = Date.now();
  const child = spawn(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(__dirname, 'windowmgr-spike.ps1'), '-ExcludePid', String(process.pid)],
    { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] }
  );
  const pending = new Map();
  const listeners = [];
  let nextId = 1;
  let resolveReady;
  const ready = new Promise((r) => { resolveReady = r; });
  child.stdout.setEncoding('utf8');
  readline.createInterface({ input: child.stdout }).on('line', (line) => {
    let msg;
    try { msg = JSON.parse(line); } catch { console.log('helper (not JSON):', line); return; }
    msg.receivedAt = Date.now();
    if (msg.ev === 'ready') return resolveReady({ ...msg, startMs: Date.now() - t0 });
    if (msg.ev) return listeners.forEach((fn) => fn(msg));
    const resolve = pending.get(msg.id);
    if (resolve) { pending.delete(msg.id); resolve(msg); }
  });
  child.stderr.on('data', (d) => process.stderr.write('helper stderr: ' + d));
  child.on('exit', (code) => console.log(`helper exited (${code})`));
  return {
    ready,
    send(cmd) {
      const id = nextId++;
      return new Promise((resolve) => {
        pending.set(id, resolve);
        child.stdin.write(JSON.stringify({ id, ...cmd }) + '\n');
      });
    },
    onEvent(fn) { listeners.push(fn); },
    stop() { child.stdin.end(); },
  };
}

// ---------- 1. helper ----------

async function checkHelper() {
  helper = startHelper();
  const ready = await helper.ready;
  check('helper starts (spawn + Add-Type compile)', ready.startMs < 3000, { startMs: ready.startMs, threadDpiAwarenessSet: ready.dpiContextSet });
  const trips = [];
  for (let i = 0; i < 30; i++) {
    const t = Date.now();
    await helper.send({ cmd: 'ping' });
    trips.push(Date.now() - t);
  }
  check('command round trip', median(trips) <= 5, { medianMs: median(trips), maxMs: Math.max(...trips) });
  helper.onEvent((ev) => {
    if (ev.ev === 'foreground') results.foregroundLatencyMs.push({ hook: ev.hookLatencyMs, pipe: ev.receivedAt - ev.epochMs });
    if (ev.ev === 'hookError') check('hook callback threw', false, ev);
  });
}

// ---------- 2. DPI ----------

async function checkDisplays() {
  const { monitors } = await helper.send({ cmd: 'monitors' });
  for (const d of screen.getAllDisplays()) {
    const phys = toLTRB(screen.dipToScreenRect(null, d.bounds));
    const match = monitors.find((m) => maxDiff(m.bounds, phys) <= 1);
    check(`display ${d.id}: dipToScreenRect matches the helper's physical monitor rect`, !!match, {
      scaleFactor: d.scaleFactor, dip: d.bounds, physical: phys, helperMonitor: match || monitors,
    });
  }
  const scales = [...new Set(screen.getAllDisplays().map((d) => d.scaleFactor))];
  check('mixed DPI present', scales.length > 1 ? true : null, { scales });
}

// ---------- 3. AppBar ----------

async function checkAppBar() {
  const primary = screen.getPrimaryDisplay();
  const before = primary.workArea;
  let metricsEvents = 0;
  screen.on('display-metrics-changed', () => { metricsEvents++; });

  panel = new BrowserWindow({
    ...primary.bounds, height: PANEL_HEIGHT_DIP,
    frame: false, resizable: false, movable: false, minimizable: false, maximizable: false,
    skipTaskbar: true, alwaysOnTop: true, focusable: false, show: false, backgroundColor: '#1e2128',
  });
  panel.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(
    '<body style="margin:0;height:100vh;display:flex;align-items:center;justify-content:center;' +
    'background:#1e2128;color:#e8e9ec;font:12px Segoe UI">Launchpad organizer spike: AppBar test bar</body>'));
  panel.hookWindowMessage(APPBAR_CALLBACK, (wParam, lParam) => {
    results.appbarCallbacks.push({ code: Number(wParam.readBigUInt64LE(0)), lParam: Number(lParam.readBigInt64LE(0)), at: Date.now() });
  });
  panel.showInactive();
  const hwnd = handleOf(panel);

  const reg = await helper.send({ cmd: 'appbarNew', hwnd, msg: APPBAR_CALLBACK });
  check('ABM_NEW sent from the helper accepts the Electron window', reg.ok === true, reg);
  if (!reg.ok) return;

  const want = screen.dipToScreenRect(panel, { ...primary.bounds, height: PANEL_HEIGHT_DIP });
  const pos = await helper.send({ cmd: 'appbarSetPos', hwnd, left: want.x, top: want.y, right: want.x + want.width, height: want.height });
  panel.setBounds(screen.screenToDipRect(panel, fromLTRB(pos.rect)));
  await sleep(1000);
  const after = screen.getPrimaryDisplay().workArea;
  const physWork = (await helper.send({ cmd: 'monitors' })).monitors.find((m) => m.primary).work;
  check('AppBar reserves the top of the work area', after.y - before.y === PANEL_HEIGHT_DIP && after.height === before.height - PANEL_HEIGHT_DIP, {
    before, after, physicalWork: physWork, appbarRect: pos.rect, displayMetricsChangedEvents: metricsEvents,
  });
}

async function checkFullscreenNotification() {
  const primary = screen.getPrimaryDisplay();
  const startCount = results.appbarCallbacks.length;
  const fs1 = new BrowserWindow({ ...primary.bounds, show: false, backgroundColor: '#101216', frame: false });
  fs1.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(
    '<body style="margin:0;height:100vh;display:grid;place-items:center;background:#101216;color:#9aa0ac;font:16px Segoe UI">' +
    'Launchpad organizer spike: fullscreen test (closes in 2 s)</body>'));
  fs1.show();
  fs1.focus();
  fs1.setFullScreen(true);
  await sleep(1800);
  const during = results.appbarCallbacks.slice(startCount);
  fs1.setFullScreen(false);
  fs1.close();
  await sleep(1200);
  const all = results.appbarCallbacks.slice(startCount);
  const entered = during.some((c) => c.code === 2 && c.lParam !== 0); // ABN_FULLSCREENAPP, TRUE
  const left = all.some((c) => c.code === 2 && c.lParam === 0);
  check('hookWindowMessage receives ABN_FULLSCREENAPP on and off', entered && left, { callbacks: all });
}

// ---------- 4. placement ----------

async function launchNotepad() {
  const existing = new Set((await helper.send({ cmd: 'find', exe: 'notepad' })).windows.map((w) => w.hwnd));
  spawn('notepad.exe', [], { detached: true, stdio: 'ignore' }).unref();
  for (let i = 0; i < 60; i++) {
    await sleep(100);
    const fresh = (await helper.send({ cmd: 'find', exe: 'notepad' })).windows.filter((w) => !existing.has(w.hwnd));
    if (fresh.length) return fresh[0];
  }
  throw new Error('Notepad window never appeared');
}

async function placeInLeftHalf(hwnd, display) {
  const wa = screen.getAllDisplays().find((d) => d.id === display.id).workArea; // re-read: the AppBar changed it
  const target = screen.dipToScreenRect(null, leftHalf(wa));
  const t = Date.now();
  const res = await helper.send({ cmd: 'place', hwnd, x: target.x, y: target.y, w: target.width, h: target.height });
  return { ...res, ms: Date.now() - t };
}

const passSummary = (res) => res.passes.map((p) => ({ borders: p.borders, maxErrorPx: p.maxErrorPx, dpiAfter: p.dpiAfter, win32Error: p.win32Error }));

async function checkPlacement() {
  const win = await launchNotepad();
  notepadHwnd = win.hwnd;
  check('Notepad window found', true, { hwnd: win.hwnd, class: win.class, dpi: win.dpi, rect: win.rect, frame: win.frame });

  const maxed = await helper.send({ cmd: 'show', hwnd: notepadHwnd, nCmdShow: 3 }); // SW_MAXIMIZE
  await sleep(300);
  const now = await helper.send({ cmd: 'describe', hwnd: notepadHwnd });
  const { monitors } = await helper.send({ cmd: 'monitors' });
  const mon = monitors.find((m) => now.frame[0] >= m.bounds[0] - 16 && now.frame[0] < m.bounds[2]);
  check('a maximized window stops below the AppBar', mon && Math.abs(now.frame[1] - mon.work[1]) <= 1, {
    maximizedFrame: now.frame, maximizedRect: now.rect, monitorWork: mon && mon.work, zoomed: maxed.zoomed,
  });

  for (const d of screen.getAllDisplays()) {
    const res = await placeInLeftHalf(notepadHwnd, d);
    check(`place Notepad in the left half of display ${d.id} (${Math.round(d.scaleFactor * 100)}%)`, res.exact, {
      ms: res.ms, restoredFirst: res.restored, target: res.target, frame: res.frame, passes: passSummary(res),
    });
  }

  // Ends up back on the primary display, maximized, then placed: the
  // restore-then-place path the drag handler needs after Aero Snap.
  await helper.send({ cmd: 'show', hwnd: notepadHwnd, nCmdShow: 3 });
  await sleep(300);
  const res = await placeInLeftHalf(notepadHwnd, screen.getPrimaryDisplay());
  check('a maximized window is restored, then placed', res.restored && res.exact, { ms: res.ms, target: res.target, frame: res.frame, passes: passSummary(res) });
}

async function probeMovable() {
  const { windows } = await helper.send({ cmd: 'probe' });
  const blocked = windows.filter((w) => !w.movable);
  check('windows SetWindowPos refuses (UIPI)', blocked.length ? true : null, {
    probed: windows.length,
    blocked,
    elevatedButMovable: windows.filter((w) => w.movable && w.elevated === true),
    elevationUnknown: windows.filter((w) => w.elevated === null).map((w) => w.exe),
  });
}

// ---------- 5. drags (manual) ----------

async function watchDrags(seconds) {
  console.log(`\nWATCH  ${seconds} s: drag the Notepad window by its title bar onto each monitor, and once all the way to the top edge.`);
  let track = null;
  let poll = null;
  helper.onEvent(async (ev) => {
    if (ev.ev === 'moveStart') {
      track = {
        exe: ev.exe, title: ev.title, hwnd: ev.hwnd, hit: ev.hit, startedAt: ev.receivedAt,
        start: { hookLatencyMs: ev.hookLatencyMs, pipeMs: ev.receivedAt - ev.epochMs },
        cursorSamples: 0, minCursorYFromDisplayTop: Infinity,
      };
      clearInterval(poll);
      poll = setInterval(() => {
        const p = screen.getCursorScreenPoint();
        const d = screen.getDisplayNearestPoint(p);
        track.cursorSamples++;
        track.minCursorYFromDisplayTop = Math.min(track.minCursorYFromDisplayTop, p.y - d.bounds.y);
      }, 16);
      return;
    }
    if (ev.ev === 'moveEnd') {
      clearInterval(poll);
      if (!track || track.hwnd !== ev.hwnd) return;
      const drag = {
        ...track,
        end: { hookLatencyMs: ev.hookLatencyMs, pipeMs: ev.receivedAt - ev.epochMs },
        durationMs: ev.receivedAt - track.startedAt,
        cursorPollHz: Math.round(track.cursorSamples / ((ev.receivedAt - track.startedAt) / 1000)),
        sizeChanged: ev.sizeChanged, zoomedAtEnd: ev.zoomed, frameAtEnd: ev.frame, after: [],
      };
      delete drag.startedAt;
      delete drag.cursorSamples;
      track = null;
      results.drags.push(drag);
      if (ev.hwnd === notepadHwnd && drag.hit === HTCAPTION) {
        const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
        const t = Date.now();
        const res = await placeInLeftHalf(notepadHwnd, display);
        drag.placed = { display: display.id, msFromMoveEnd: Date.now() - ev.receivedAt, placeMs: Date.now() - t, exact: res.exact, restoredFirst: res.restored, target: res.target, frame: res.frame };
      }
      console.log('DRAG  ' + JSON.stringify(drag));
      return;
    }
    if (ev.ev === 'afterMove') {
      const drag = [...results.drags].reverse().find((d) => d.hwnd === ev.hwnd);
      if (!drag) return;
      const entry = { delayMs: ev.delayMs, zoomed: ev.zoomed, frame: ev.frame };
      if (drag.placed) entry.stillOnTarget = maxDiff(ev.frame, drag.placed.target) <= 1;
      drag.after.push(entry);
      console.log(`      after ${ev.delayMs} ms: ${JSON.stringify(entry)}`);
    }
  });
  await sleep(seconds * 1000);
}

// ---------- run ----------

let finishing = false;
async function finish(code) {
  if (finishing) return;
  finishing = true;
  try {
    if (panel && helper) {
      const before = screen.getPrimaryDisplay().workArea;
      await helper.send({ cmd: 'appbarRemove', hwnd: handleOf(panel) });
      await sleep(800);
      const after = screen.getPrimaryDisplay().workArea;
      check('ABM_REMOVE gives the space back', after.y < before.y, { before, after });
    }
    if (notepadHwnd && helper) await helper.send({ cmd: 'close', hwnd: notepadHwnd });
  } catch (err) {
    console.error(err);
  }
  const fl = results.foregroundLatencyMs;
  if (fl.length) results.foregroundSummary = { events: fl.length, medianHookMs: median(fl.map((x) => x.hook)), medianPipeMs: median(fl.map((x) => x.pipe)) };
  fs.writeFileSync(OUT_FILE, JSON.stringify(results, null, 2));
  console.log(`\nWrote ${OUT_FILE}`);
  if (helper) helper.stop();
  app.exit(code);
}

app.on('window-all-closed', () => {}); // keep running between test windows
process.on('SIGINT', () => finish(130));

app.whenReady().then(async () => {
  try {
    await checkHelper();
    await checkDisplays();
    await checkAppBar();
    await checkFullscreenNotification();
    await checkPlacement();
    await probeMovable();
    if (WATCH_SECONDS > 0) await watchDrags(WATCH_SECONDS);
    await finish(results.checks.some((c) => c.pass === false) ? 1 : 0);
  } catch (err) {
    console.error(err);
    await finish(1);
  }
});
