// Draws the organizer panel for one display: every layout as a small
// thumbnail whose zones move the focused window when clicked, that window's
// title, and a clock. Main (lib/organizer.js) sends what to show. The panel
// window never takes focus, so the user's own window stays in front while
// they click here, and that's the window that moves.

// Loaded from file:// like the main window, so it shares its localStorage
// and follows theme, accent and look changes made there as they happen.
function applyLook() {
  try {
    const root = document.documentElement;
    root.setAttribute('data-theme', localStorage.getItem('launchpad-theme') === 'light' ? 'light' : 'dark');
    const accent = localStorage.getItem('launchpad-accent');
    if (accent) root.style.setProperty('--accent', accent);
    else root.style.removeProperty('--accent');
    const look = localStorage.getItem('launchpad-panel-style');
    if (look === 'translucent' || look === 'transparent') root.setAttribute('data-style', look);
    else root.removeAttribute('data-style');
  } catch {}
}
applyLook();
window.addEventListener('storage', applyLook);

const barEl = document.getElementById('bar');
const layoutsEl = document.getElementById('layouts');
const iconEl = document.getElementById('focusedIcon');
const titleEl = document.getElementById('focusedTitle');
const clockEl = document.getElementById('clock');

const THUMB_HEIGHT = 20; // px; the width follows the shape of the display's work area
const MESSAGE_MS = 3000;

let state = null; // { layouts, focused: { title, iconDataUrl, snappable } | null, helperUp, aspect, height }
let layoutsKey = '';
let messageTimer = null;

const canSnap = () => !!(state && state.helperUp && state.focused && state.focused.snappable);

// Zones sit on the layout's own grid, so CSS grid lays them out exactly as
// lib/zones.js does on screen, just without the gaps.
function buildLayouts(layouts, aspect) {
  const width = `${Math.round(THUMB_HEIGHT * aspect)}px`;
  layoutsEl.replaceChildren(
    ...layouts.map((layout) => {
      const thumb = document.createElement('div');
      thumb.className = 'thumb';
      thumb.style.width = width;
      thumb.style.gridTemplateColumns = `repeat(${layout.cols}, 1fr)`;
      thumb.style.gridTemplateRows = `repeat(${layout.rows}, 1fr)`;
      for (const z of layout.zones) {
        const target = { layoutId: layout.id, zoneId: z.id };
        const zone = document.createElement('button');
        zone.type = 'button';
        zone.className = 'zone';
        zone.title = `${layout.name}: ${z.name}`;
        zone.style.gridColumn = `${z.x + 1} / span ${z.w}`;
        zone.style.gridRow = `${z.y + 1} / span ${z.h}`;
        zone.addEventListener('mouseenter', () => {
          if (canSnap()) window.panelAPI.hover(target);
        });
        zone.addEventListener('mouseleave', () => window.panelAPI.hover(null));
        zone.addEventListener('click', () => snap(target));
        thumb.appendChild(zone);
      }
      return thumb;
    })
  );
}

async function snap(target) {
  if (!canSnap()) {
    showMessage(state && !state.helperUp ? 'The organizer is starting…' : 'Click the window you want to move first.');
    return;
  }
  window.panelAPI.hover(null); // the window is about to fill the preview
  const result = await window.panelAPI.snap(target.layoutId, target.zoneId);
  if (!result.ok) showMessage(result.error);
}

function showFocused() {
  const f = state && state.focused;
  titleEl.classList.remove('message');
  titleEl.textContent = f ? f.title : '';
  const icon = f && f.iconDataUrl;
  iconEl.classList.toggle('hidden', !icon);
  if (icon) iconEl.src = icon;
}

// Shown where the title goes for a few seconds: Launchpad's own window is
// usually hidden, so a dialog there wouldn't be seen.
function showMessage(text) {
  clearTimeout(messageTimer);
  titleEl.textContent = text;
  titleEl.classList.add('message');
  iconEl.classList.add('hidden');
  messageTimer = setTimeout(() => {
    messageTimer = null;
    showFocused();
  }, MESSAGE_MS);
}

function render(next) {
  state = next;
  // The window can be taller than the panel (see setPanelBounds in
  // lib/panel.js); anything below this height is cut off.
  barEl.style.height = `${next.height}px`;
  // Rebuilt only when the layouts or the display's shape change, so a focus
  // change doesn't flicker the thumbnails or drop a hover.
  const key = JSON.stringify([next.layouts, next.aspect.toFixed(3)]);
  if (key !== layoutsKey) {
    layoutsKey = key;
    buildLayouts(next.layouts, next.aspect);
  }
  layoutsEl.classList.toggle('idle', !canSnap());
  if (!messageTimer) showFocused();
}

window.panelAPI.onState(render);

function updateClock() {
  const now = new Date();
  const date = document.createElement('span');
  date.className = 'clock-date';
  date.textContent = now.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
  const time = document.createElement('span');
  time.textContent = now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  clockEl.replaceChildren(date, time);
}
updateClock();
setInterval(updateClock, 1000);
