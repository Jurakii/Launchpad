// Draws the scenes menu under a panel's scenes button: the saved scenes
// (click one, or pick it with the arrow keys and Enter, to apply it), then
// "Save current arrangement" and "Manage scenes…". Esc closes it, and so
// does clicking anywhere else (main sees the window lose the focus). Main
// sends the list and does the work; this page only draws and reports.

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
const listEl = document.getElementById('list');
const emptyEl = document.getElementById('empty');

document.getElementById('saveBtn').addEventListener('click', () => window.scenesMenuAPI.save());
document.getElementById('manageBtn').addEventListener('click', () => window.scenesMenuAPI.manage());

// { scenes: [{ id, name, count, island }] }
function render(state) {
  const scenes = (state && state.scenes) || [];
  listEl.replaceChildren(
    ...scenes.map((s) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'item';
      btn.setAttribute('role', 'menuitem');
      btn.title = `Apply "${s.name}": put its ${s.count} window${s.count === 1 ? '' : 's'} back in place`;
      const name = document.createElement('span');
      name.className = 'scene-name';
      name.textContent = s.name;
      btn.appendChild(name);
      if (s.island) {
        const tag = document.createElement('span');
        tag.className = 'tag';
        tag.textContent = 'island';
        btn.appendChild(tag);
      }
      const meta = document.createElement('span');
      meta.className = 'scene-meta';
      meta.textContent = `${s.count} window${s.count === 1 ? '' : 's'}`;
      btn.appendChild(meta);
      btn.addEventListener('click', () => window.scenesMenuAPI.apply(s.id));
      return btn;
    })
  );
  emptyEl.classList.toggle('hidden', scenes.length > 0);
  window.scenesMenuAPI.resize(menuEl.getBoundingClientRect().height);
  // The first item, so the keyboard works straight away.
  requestAnimationFrame(() => {
    const first = menuEl.querySelector('.item');
    if (first) first.focus();
  });
}

window.scenesMenuAPI.onState(render);

// Arrows move between items (wrapping); Enter and Space are the buttons' own.
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    e.preventDefault();
    window.scenesMenuAPI.close();
    return;
  }
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  e.preventDefault();
  const items = [...menuEl.querySelectorAll('.item')];
  const i = items.indexOf(document.activeElement);
  const next = e.key === 'ArrowDown' ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
  items[next].focus();
});
