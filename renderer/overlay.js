// Draws the drag overlay for one display: the layout strip along the top
// and, when a zone is hovered, a full-size preview of where the window will
// go. Hovering a zone in the organizer panel shows the preview alone (no
// strip). Main sends every rect already worked out (lib/strip.js) and
// relative to this window, so this file only draws. It never gets input: the
// window ignores the mouse, and a dragged window has it captured anyway.

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

const previewEl = document.getElementById('preview');
const stripEl = document.getElementById('strip');

// Zones are drawn this far inside their hit rect, which leaves a visible
// line between neighbours without leaving gaps the cursor could fall into.
const ZONE_INSET = 2;
const LABEL_OFFSET = 4;

function box(className, r, inset = 0) {
  const el = document.createElement('div');
  el.className = className;
  el.style.left = `${r.x + inset}px`;
  el.style.top = `${r.y + inset}px`;
  el.style.width = `${Math.max(0, r.width - 2 * inset)}px`;
  el.style.height = `${Math.max(0, r.height - 2 * inset)}px`;
  return el;
}

function render(state) {
  previewEl.classList.toggle('hidden', !(state && state.preview));
  stripEl.classList.toggle('hidden', !(state && state.strip));
  if (state && state.preview) {
    const p = state.preview;
    Object.assign(previewEl.style, { left: `${p.x}px`, top: `${p.y}px`, width: `${p.width}px`, height: `${p.height}px` });
  }
  if (!state || !state.strip) {
    stripEl.replaceChildren();
    return;
  }

  const hovered = state.hovered;
  const parts = [box('strip-bg', state.strip.rect)];
  for (const item of state.strip.items) {
    const isHoveredLayout = hovered && hovered.layoutId === item.layoutId;
    for (const z of item.zones) {
      const zoneEl = box('zone', z.rect, ZONE_INSET);
      if (isHoveredLayout && hovered.zoneId === z.zoneId) zoneEl.classList.add('hovered');
      parts.push(zoneEl);
    }
    const label = box('label', { x: item.thumb.x, y: item.thumb.y + item.thumb.height + LABEL_OFFSET, width: item.thumb.width, height: 16 });
    label.textContent = item.name;
    if (isHoveredLayout) label.classList.add('hovered');
    parts.push(label);
  }
  stripEl.replaceChildren(...parts);
}

window.overlayAPI.onState(render);
