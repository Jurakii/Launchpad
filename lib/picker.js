// What the window picker (lib/window-picker.js) is for, kept free of
// Electron so it can be tested: a click on a panel's zone or grid button
// moves the window the user was in when there is one, and otherwise opens
// the picker so they can choose one from the list instead of hitting a dead
// end (the usual case when Launchpad itself is in front).

const MAX_ENTRIES = 30;

// 'snap' when target (a foreground event, or null) can be moved directly,
// 'pick' when the picker should open instead, or 'wait' while the helper
// that lists and moves windows isn't running yet.
function panelAction(target, helperUp) {
  if (!helperUp) return 'wait';
  return target && target.snappable ? 'snap' : 'pick';
}

// The helper's window list (windowmgr.listWindows(): top of the z-order
// first) as picker rows: { hwnd, exe, title, minimized }. Anything without
// a usable handle or title is dropped, each window appears once, and the
// list is capped so the menu stays a menu.
function pickerEntries(windows) {
  const seen = new Set();
  const entries = [];
  for (const w of Array.isArray(windows) ? windows : []) {
    if (!w || !Number.isSafeInteger(w.hwnd) || w.hwnd === 0 || seen.has(w.hwnd)) continue;
    const title = typeof w.title === 'string' ? w.title.trim() : '';
    if (!title) continue;
    seen.add(w.hwnd);
    entries.push({ hwnd: w.hwnd, exe: typeof w.exe === 'string' ? w.exe : '', title, minimized: !!w.minimized });
    if (entries.length === MAX_ENTRIES) break;
  }
  return entries;
}

// The row the picker reported (by handle), or null if it isn't one it was
// shown - the page only ever sends back what main gave it.
function findEntry(entries, hwnd) {
  return (Array.isArray(entries) && entries.find((e) => e.hwnd === hwnd)) || null;
}

module.exports = { panelAction, pickerEntries, findEntry, MAX_ENTRIES };
