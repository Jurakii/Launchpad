// Draws the organizer panel for one display: every layout as a small
// thumbnail whose zones move the focused window when clicked, that window's
// title, a button for the grid placer, and a clock. Main (lib/organizer.js)
// sends what to show. The panel window never takes focus, so the user's own
// window stays in front while they click here, and that's the window that
// moves.
//
// The panel on the display Launchpad's main window is on is also its top
// bar: main adds `launchpad` to the state, and the window's breadcrumb and
// toolbar buttons show here instead of in the window. Clicks on them go
// back to the main window (lib/panel.js, main.js).

// Loaded from file:// like the main window, so it shares its localStorage
// and follows theme, accent, top bar look and the add-button setting as
// they change there.
function applyLook() {
  try {
    const root = document.documentElement;
    root.setAttribute('data-theme', localStorage.getItem('launchpad-theme') === 'light' ? 'light' : 'dark');
    const accent = localStorage.getItem('launchpad-accent');
    if (accent) root.style.setProperty('--accent', accent);
    else root.style.removeProperty('--accent');
    const look = localStorage.getItem('launchpad-topbar-style');
    if (look === 'translucent' || look === 'transparent') root.setAttribute('data-style', look);
    else root.removeAttribute('data-style');
    root.toggleAttribute('data-hide-add-buttons', localStorage.getItem('launchpad-hide-add-buttons') === '1');
  } catch {}
}
applyLook();
window.addEventListener('storage', applyLook);

// The same icon files as the main window's toolbar, inline so they take the
// button's colour (see ICON_FILES in renderer.js).
function loadIcon(name) {
  try {
    const xhr = new XMLHttpRequest();
    xhr.open('GET', `icons/${name}.svg`, false); // synchronous - tiny local files
    xhr.send(null);
    if (xhr.status === 200 || xhr.status === 0) return xhr.responseText;
  } catch {}
  return '';
}
const ICONS = { settings: loadIcon('settings'), fullscreenEnter: loadIcon('fullscreen-enter'), fullscreenExit: loadIcon('fullscreen-exit') };

const barEl = document.getElementById('bar');
const layoutsEl = document.getElementById('layouts');
const breadcrumbEl = document.getElementById('breadcrumb');
const iconEl = document.getElementById('focusedIcon');
const titleEl = document.getElementById('focusedTitle');
const gridBtn = document.getElementById('gridBtn');
const fullscreenBtn = document.getElementById('fullscreenBtn');
const clockEl = document.getElementById('clock');

document.getElementById('settingsBtn').innerHTML = ICONS.settings;

const THUMB_HEIGHT = 20; // px; the width follows the shape of the display's work area
const MESSAGE_MS = 3000;

// { layouts, focused: { title, iconDataUrl, snappable } | null, helperUp, gridHotkey, aspect, height,
//   launchpad: { crumbs: [{ id, name }], fullscreen } | null }
let state = null;
let layoutsKey = '';
let crumbsKey = '';
let messageTimer = null;

const canSnap = () => !!(state && state.helperUp && state.focused && state.focused.snappable);

// Why nothing happened, when there's nothing to move.
const cantSnapMessage = () => (state && !state.helperUp ? 'The organizer is starting…' : 'Click the window you want to move first.');

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
        zone.title = `${layout.name}: ${z.name || z.id}`;
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
    showMessage(cantSnapMessage());
    return;
  }
  window.panelAPI.hover(null); // the window is about to fill the preview
  const result = await window.panelAPI.snap(target.layoutId, target.zoneId);
  if (!result.ok) showMessage(result.error);
}

// Opens the scenes menu under this button, or closes it if it's open here.
// It works whatever window is in front: applying a scene moves windows of
// its own choosing.
const scenesBtn = document.getElementById('scenesBtn');
scenesBtn.addEventListener('click', () => {
  const r = scenesBtn.getBoundingClientRect();
  window.panelAPI.openScenes({ x: window.screenX + r.left, y: window.screenY, width: r.width, height: barEl.offsetHeight });
});

// From main, e.g. what applying a scene did. Long ones stay up longer.
window.panelAPI.onMessage((text) => showMessage(text, Math.min(8000, Math.max(MESSAGE_MS, text.length * 60))));

// Opens the grid placer on this display for the focused window, or closes
// it if it's already open here.
gridBtn.addEventListener('click', async () => {
  if (!canSnap()) {
    showMessage(cantSnapMessage());
    return;
  }
  const result = await window.panelAPI.openGrid();
  if (!result.ok) showMessage(result.error);
});

// ---------- Launchpad's toolbar ----------

const toolbarAction = (action, detail = {}) => window.panelAPI.launchpad(action, detail);

for (const [id, action] of [
  ['layoutsBtn', 'layouts'],
  ['settingsBtn', 'settings'],
  ['newIslandBtn', 'newIsland'],
  ['addAppBtn', 'addApp'],
  ['fullscreenBtn', 'fullscreen'],
]) {
  document.getElementById(id).addEventListener('click', () => toolbarAction(action));
}

const isLaunchpadDrag = (e) => e.dataTransfer.types.includes('application/x-launchpad-id');

// The same breadcrumb as the main window's. Dropping apps or islands
// dragged from the window onto a crumb moves them there, and the current
// island's crumb has the island's menu, both as in the window.
function buildCrumbs(crumbs) {
  breadcrumbEl.replaceChildren();
  crumbs.forEach((c, i) => {
    if (i > 0) {
      const sep = document.createElement('span');
      sep.className = 'crumb-sep';
      sep.textContent = '›';
      breadcrumbEl.appendChild(sep);
    }
    const last = i === crumbs.length - 1;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'crumb' + (last ? ' current' : '');
    btn.textContent = c.name;
    btn.addEventListener('click', () => toolbarAction('crumb', { id: c.id }));
    if (last && c.id) {
      btn.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        toolbarAction('crumbMenu', { id: c.id, x: e.screenX, y: e.screenY });
      });
    }
    btn.addEventListener('dragover', (e) => {
      if (!isLaunchpadDrag(e)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      btn.classList.add('drag-over');
    });
    btn.addEventListener('dragleave', () => btn.classList.remove('drag-over'));
    btn.addEventListener('drop', (e) => {
      if (!isLaunchpadDrag(e)) return;
      e.preventDefault();
      btn.classList.remove('drag-over');
      const group = e.dataTransfer.getData('application/x-launchpad-ids');
      const ids = group ? JSON.parse(group) : [e.dataTransfer.getData('application/x-launchpad-id')];
      toolbarAction('drop', { id: c.id, ids });
    });
    breadcrumbEl.appendChild(btn);
  });
}

function renderLaunchpad(launchpad) {
  document.documentElement.toggleAttribute('data-launchpad', !!launchpad);
  if (!launchpad) return;
  const key = JSON.stringify(launchpad.crumbs);
  if (key !== crumbsKey) {
    crumbsKey = key;
    buildCrumbs(launchpad.crumbs);
  }
  fullscreenBtn.innerHTML = launchpad.fullscreen ? ICONS.fullscreenExit : ICONS.fullscreenEnter;
  fullscreenBtn.title = launchpad.fullscreen ? 'Exit fullscreen' : 'Enter fullscreen';
}

// ---------- the focused window ----------

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
function showMessage(text, ms = MESSAGE_MS) {
  clearTimeout(messageTimer);
  titleEl.textContent = text;
  titleEl.title = text; // the whole of it, should it not fit
  titleEl.classList.add('message');
  iconEl.classList.add('hidden');
  messageTimer = setTimeout(() => {
    messageTimer = null;
    titleEl.title = '';
    showFocused();
  }, ms);
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
  gridBtn.classList.toggle('idle', !canSnap());
  const keys = next.gridHotkey ? ` (${next.gridHotkey.replace('CommandOrControl', 'Ctrl')})` : '';
  gridBtn.title = `Grid placer${keys}: size the window to any block of cells`;
  renderLaunchpad(next.launchpad);
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
