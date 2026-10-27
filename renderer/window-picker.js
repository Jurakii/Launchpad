// Draws the window picker under a panel's zone or grid button: the open
// windows (click one, or pick it with the arrow keys and Enter, to move it
// where the user clicked). Esc closes it, and so does clicking anywhere
// else (main sees the window lose the focus). Main sends the list and does
// the work; this page only draws and reports.

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

const menuEl = document.getElementById('menu');
const headingEl = document.getElementById('heading');
const listEl = document.getElementById('list');
const emptyEl = document.getElementById('empty');
menuEl.tabIndex = -1;

// { heading, windows: [{ hwnd, title, iconDataUrl, minimized }] }
function render(state) {
  const windows = (state && state.windows) || [];
  headingEl.textContent = (state && state.heading) || 'Pick a window';
  listEl.replaceChildren(
    ...windows.map((w) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'item';
      btn.setAttribute('role', 'menuitem');
      btn.title = w.title;
      const icon = document.createElement(w.iconDataUrl ? 'img' : 'span');
      icon.className = 'win-icon';
      if (w.iconDataUrl) {
        icon.src = w.iconDataUrl;
        icon.alt = '';
      }
      btn.appendChild(icon);
      const name = document.createElement('span');
      name.className = 'win-title';
      name.textContent = w.title;
      btn.appendChild(name);
      if (w.minimized) {
        const tag = document.createElement('span');
        tag.className = 'tag';
        tag.textContent = 'minimized';
        btn.appendChild(tag);
      }
      btn.addEventListener('click', () => window.pickerAPI.pick(w.hwnd));
      return btn;
    })
  );
  emptyEl.classList.toggle('hidden', windows.length > 0);
  window.pickerAPI.resize(menuEl.getBoundingClientRect().height);
  // Nothing is chosen up front, so Enter can't move a window the user
  // didn't pick: the menu itself takes the focus and an arrow key picks
  // the first (or last) row.
  requestAnimationFrame(() => menuEl.focus());
}

window.pickerAPI.onState(render);

// Arrows move between windows (wrapping); Enter and Space are the buttons'
// own, so they do nothing until a row has the focus.
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    e.preventDefault();
    window.pickerAPI.close();
    return;
  }
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  e.preventDefault();
  const items = [...menuEl.querySelectorAll('.item')];
  if (!items.length) return;
  const i = items.indexOf(document.activeElement);
  const down = e.key === 'ArrowDown';
  const next = i < 0 ? (down ? 0 : items.length - 1) : down ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
  items[next].focus();
});
