// Draws the grid placer over one display and lets the user pick a block of
// cells for the window being moved: drag across cells, or click one corner
// and then the opposite one; or use the arrow keys to move the selection,
// Shift+arrows to resize it, and Enter to place the window. Esc cancels.
// Main (lib/organizer.js) sends what to show and does the placing. The cell
// math is in cells.js.

// Loaded from file:// like the main window, so it shares its localStorage
// and follows theme and accent changes made there.
function applyTheme() {
  try {
    const root = document.documentElement;
    root.setAttribute('data-theme', localStorage.getItem('launchpad-theme') === 'light' ? 'light' : 'dark');
    const accent = localStorage.getItem('launchpad-accent');
    if (accent) root.style.setProperty('--accent', accent);
    else root.style.removeProperty('--accent');
  } catch {}
}
applyTheme();
window.addEventListener('storage', applyTheme);

const gridEl = document.getElementById('grid');
const infoEl = document.getElementById('info');
const infoIcon = document.getElementById('infoIcon');
const infoTitle = document.getElementById('infoTitle');
const infoSize = document.getElementById('infoSize');
const infoHint = document.getElementById('infoHint');

const HINT = 'Drag across cells, or click two corners · Arrows move · Shift+arrows resize · Enter places · Esc cancels';
const CORNER_HINT = 'Now click the opposite corner.';

let state = null; // { cols, rows, gap, area, frame, title, iconDataUrl }
let block = null; // the selection, in cells
let selectionEl = null;
let anchor = null; // the first corner, while dragging or after a click on one cell
let dragging = false;
let placing = false; // waiting for main

function render(next) {
  state = next;
  anchor = null;
  dragging = false;
  placing = false;
  gridEl.classList.toggle('hidden', !state);
  infoEl.classList.toggle('hidden', !state);
  if (!state) {
    gridEl.replaceChildren();
    return;
  }
  const { cols, rows, gap } = state;
  Object.assign(gridEl.style, {
    gridTemplateColumns: `repeat(${cols}, 1fr)`,
    gridTemplateRows: `repeat(${rows}, 1fr)`,
    gap: `${gap}px`,
    padding: `${gap}px`,
  });
  // Placed explicitly: auto-placement would steer the cells around the
  // selection, which shares their grid.
  const parts = [];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const cell = document.createElement('div');
      cell.className = 'cell';
      cell.style.gridColumn = `${x + 1}`;
      cell.style.gridRow = `${y + 1}`;
      parts.push(cell);
    }
  }
  selectionEl = document.createElement('div');
  selectionEl.className = 'selection';
  parts.push(selectionEl);
  gridEl.replaceChildren(...parts);

  infoTitle.textContent = state.title;
  infoIcon.classList.toggle('hidden', !state.iconDataUrl);
  if (state.iconDataUrl) infoIcon.src = state.iconDataUrl;
  // Starts where the window already is, so Enter alone lines it up with the grid.
  block = (state.frame && blockFromRect(state.frame, state.area, cols, rows)) || defaultBlock(cols, rows);
  showHint(HINT);
  drawSelection();
}

function drawSelection() {
  selectionEl.style.gridColumn = `${block.x + 1} / span ${block.w}`;
  selectionEl.style.gridRow = `${block.y + 1} / span ${block.h}`;
  infoSize.textContent = `${block.w} × ${block.h} of ${state.cols} × ${state.rows}`;
  infoEl.classList.toggle('bottom', block.y === 0);
}

function showHint(text, isError = false) {
  infoHint.textContent = text;
  infoHint.classList.toggle('error', isError);
}

function cellAt(e) {
  const r = gridEl.getBoundingClientRect();
  const x = Math.floor(((e.clientX - r.left) / r.width) * state.cols);
  const y = Math.floor(((e.clientY - r.top) / r.height) * state.rows);
  return { x: Math.min(state.cols - 1, Math.max(0, x)), y: Math.min(state.rows - 1, Math.max(0, y)) };
}

async function place() {
  if (placing) return;
  placing = true;
  anchor = null;
  dragging = false;
  let result;
  try {
    result = await window.gridAPI.place(block);
  } catch (err) {
    result = { ok: false, error: String((err && err.message) || err) };
  }
  placing = false;
  // On success main closes the grid.
  if (!result.ok) showHint(result.error, true);
}

gridEl.addEventListener('pointerdown', (e) => {
  if (!state || placing || e.button !== 0) return;
  e.preventDefault();
  const cell = cellAt(e);
  if (anchor && !dragging) {
    block = cellSpan(anchor, cell); // the second corner
    drawSelection();
    place();
    return;
  }
  anchor = cell;
  dragging = true;
  block = cellSpan(cell, cell);
  gridEl.setPointerCapture(e.pointerId);
  showHint(HINT);
  drawSelection();
});

gridEl.addEventListener('pointermove', (e) => {
  if (!state || !anchor || placing) return;
  const next = cellSpan(anchor, cellAt(e));
  if (next.x === block.x && next.y === block.y && next.w === block.w && next.h === block.h) return;
  block = next;
  drawSelection();
});

gridEl.addEventListener('pointerup', (e) => {
  if (!dragging) return;
  dragging = false;
  const cell = cellAt(e);
  if (cell.x === anchor.x && cell.y === anchor.y) {
    showHint(CORNER_HINT); // a click, not a drag: wait for the other corner
    return;
  }
  block = cellSpan(anchor, cell);
  drawSelection();
  place();
});

gridEl.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  window.gridAPI.cancel();
});

const ARROWS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

document.addEventListener('keydown', (e) => {
  if (!state) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    window.gridAPI.cancel();
    return;
  }
  if (placing) return;
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault();
    place();
    return;
  }
  const step = ARROWS[e.key];
  if (!step) return;
  e.preventDefault();
  anchor = null; // the keyboard takes over from a half-made mouse selection
  dragging = false;
  showHint(HINT);
  block = e.shiftKey ? resizeBlock(block, step[0], step[1], state.cols, state.rows) : moveBlock(block, step[0], step[1], state.cols, state.rows);
  drawSelection();
});

window.gridAPI.onState(render);
