// Applied first, before anything else runs, to minimize the flash if the
// user has picked light theme (can't do this via an inline <script> - CSP
// blocks inline scripts, so it lives here instead of index.html's <head>).
try {
  if (localStorage.getItem('launchpad-theme') === 'light') {
    document.documentElement.setAttribute('data-theme', 'light');
  }
  const savedAccent = localStorage.getItem('launchpad-accent');
  if (savedAccent) document.documentElement.style.setProperty('--accent', savedAccent);
  const savedWallpaper = localStorage.getItem('launchpad-wallpaper');
  if (savedWallpaper) document.documentElement.style.setProperty('--wallpaper', `url("${savedWallpaper}")`);
  const savedTaskbarStyle = localStorage.getItem('launchpad-taskbar-style');
  if (savedTaskbarStyle && savedTaskbarStyle !== 'solid') {
    document.getElementById('taskbar').setAttribute('data-style', savedTaskbarStyle);
  }
  const savedTopbarStyle = localStorage.getItem('launchpad-topbar-style');
  if (savedTopbarStyle && savedTopbarStyle !== 'solid') {
    document.querySelector('.toolbar').setAttribute('data-style', savedTopbarStyle);
  }
  if (localStorage.getItem('launchpad-hide-add-buttons') === '1') {
    document.documentElement.setAttribute('data-hide-add-buttons', '1');
  }
} catch {}

const grid = document.getElementById('grid');
const canvas = document.getElementById('canvas');
const emptyState = document.getElementById('empty');
const emptyTitle = document.getElementById('emptyTitle');
const emptySubtitle = document.getElementById('emptySubtitle');
const breadcrumbEl = document.getElementById('breadcrumb');
const addBtn = document.getElementById('addBtn');
const newFolderBtn = document.getElementById('newFolderBtn');
const settingsBtn = document.getElementById('settingsBtn');

const modalOverlay = document.getElementById('modalOverlay');
const modalTitle = document.getElementById('modalTitle');
const appForm = document.getElementById('appForm');
const cancelBtn = document.getElementById('cancelBtn');
const deleteBtn = document.getElementById('deleteBtn');

const iconPreview = document.getElementById('iconPreview');
const chooseIconBtn = document.getElementById('chooseIconBtn');
const autoIconBtn = document.getElementById('autoIconBtn');

const fName = document.getElementById('fName');
const fType = document.getElementById('fType');
const targetRow = document.getElementById('targetRow');
const targetLabel = document.getElementById('targetLabel');
const fTarget = document.getElementById('fTarget');
const browseBtn = document.getElementById('browseBtn');
const browseFolderBtn = document.getElementById('browseFolderBtn');
const urlRow = document.getElementById('urlRow');
const fUrl = document.getElementById('fUrl');
const scriptRow = document.getElementById('scriptRow');
const fInterpreter = document.getElementById('fInterpreter');
const fArgs = document.getElementById('fArgs');
const fCwdScript = document.getElementById('fCwdScript');
const commandRow = document.getElementById('commandRow');
const fCommand = document.getElementById('fCommand');
const fCwdCommand = document.getElementById('fCwdCommand');

const contextMenu = document.getElementById('contextMenu');

const nameModalOverlay = document.getElementById('nameModalOverlay');
const nameModalTitle = document.getElementById('nameModalTitle');
const nameForm = document.getElementById('nameForm');
const nameInput = document.getElementById('nameInput');
const nameCancelBtn = document.getElementById('nameCancelBtn');
const colorRow = document.getElementById('colorRow');
const colorSwatches = document.getElementById('colorSwatches');
const customIslandColorInput = document.getElementById('customIslandColorInput');

const moveModalOverlay = document.getElementById('moveModalOverlay');
const moveList = document.getElementById('moveList');
const moveCancelBtn = document.getElementById('moveCancelBtn');

const dialogOverlay = document.getElementById('dialogOverlay');
const dialogMessage = document.getElementById('dialogMessage');
const dialogCancelBtn = document.getElementById('dialogCancelBtn');
const dialogOkBtn = document.getElementById('dialogOkBtn');

const settingsModalOverlay = document.getElementById('settingsModalOverlay');
const settingsCloseBtn = document.getElementById('settingsCloseBtn');
const autoLaunchToggle = document.getElementById('autoLaunchToggle');
const themeToggle = document.getElementById('themeToggle');
const fullscreenToggle = document.getElementById('fullscreenToggle');
const minimizeToTrayToggle = document.getElementById('minimizeToTrayToggle');
const showAddButtonsToggle = document.getElementById('showAddButtonsToggle');
const accentColorInput = document.getElementById('accentColorInput');
const accentResetBtn = document.getElementById('accentResetBtn');
const taskbarStyleSelect = document.getElementById('taskbarStyleSelect');
const topbarStyleSelect = document.getElementById('topbarStyleSelect');
const wallpaperPickBtn = document.getElementById('wallpaperPickBtn');
const wallpaperResetBtn = document.getElementById('wallpaperResetBtn');
const wallpaperFileInput = document.getElementById('wallpaperFileInput');
const toolbarEl = document.querySelector('.toolbar');
const taskbarEl = document.getElementById('taskbar');
const hotkeyBtn = document.getElementById('hotkeyBtn');
const versionLabel = document.getElementById('versionLabel');
const detectIconsBtn = document.getElementById('detectIconsBtn');
const exportBtn = document.getElementById('exportBtn');
const importBtn = document.getElementById('importBtn');

const pinnedRow = document.getElementById('pinnedRow');
const pinnedEmpty = document.getElementById('pinnedEmpty');
const taskbarDivider = document.getElementById('taskbarDivider');
const openWindowsRow = document.getElementById('openWindowsRow');
const pinInsertLine = document.getElementById('pinInsertLine');
const taskbarSearch = document.getElementById('taskbarSearch');
const searchResults = document.getElementById('searchResults');
const recentBtn = document.getElementById('recentBtn');
const recentPopout = document.getElementById('recentPopout');

const volumeBtn = document.getElementById('volumeBtn');
const volumePopout = document.getElementById('volumePopout');
const muteBtn = document.getElementById('muteBtn');
const volumeSlider = document.getElementById('volumeSlider');
const volumePct = document.getElementById('volumePct');
const outputDeviceSelect = document.getElementById('outputDeviceSelect');
const sessionMixerList = document.getElementById('sessionMixerList');
const usbBtn = document.getElementById('usbBtn');
const usbPopout = document.getElementById('usbPopout');
const usbList = document.getElementById('usbList');
const bluetoothBtn = document.getElementById('bluetoothBtn');
const bluetoothPopout = document.getElementById('bluetoothPopout');
const bluetoothList = document.getElementById('bluetoothList');
const powerBtn = document.getElementById('powerBtn');
const powerPopout = document.getElementById('powerPopout');
const powerSleepBtn = document.getElementById('powerSleepBtn');
const powerRestartBtn = document.getElementById('powerRestartBtn');
const powerShutdownBtn = document.getElementById('powerShutdownBtn');
const fullscreenBtn = document.getElementById('fullscreenBtn');
const networkBtn = document.getElementById('networkBtn');
const networkPopout = document.getElementById('networkPopout');
const networkList = document.getElementById('networkList');
const networkSettingsBtn = document.getElementById('networkSettingsBtn');
const clockTime = document.getElementById('clockTime');
const clockDate = document.getElementById('clockDate');

const PLACEHOLDER_ICON =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect width="48" height="48" rx="10" fill="#2b2f38"/><text x="24" y="30" font-size="20" text-anchor="middle" fill="#9aa0ac" font-family="sans-serif">?</text></svg>'
  );

const ISLAND_COLORS = ['#5b8cff', '#ff8a5b', '#5bd6ff', '#ffd75b', '#ff5b8a', '#8a5bff', '#5bffb0', '#9aa0ac'];

// Canvas used at every level: fixed square cell size, snap-to-grid (not
// true freeform). Mirrors lib/layout.js's footprint rule - keep in sync.
const CELL = 112;
const CARD_MARGIN = 14; // px shaved off each side so cards don't touch neighbors
// Must match .island-card-header / .island-card-body in styles.css - used
// to work out how many icons actually fit rather than guessing.
const ISLAND_HEADER_H = 43;
const ISLAND_BODY_PAD = 24;
const ISLAND_BODY_GAP = 8;
const ISLAND_NAME_LABEL_H = 14; // extra row height reserved when showNames is on
function footprintOf(item) {
  if (item.kind === 'island' && item.collapsed === false) {
    return { w: item.w || 3, h: item.h || 2 };
  }
  return { w: 1, h: 1 };
}

let apps = [];
let currentFolderId = null;
let lastRenderedFolderId; // sentinel (undefined) so the very first render doesn't need special-casing
let currentItems = [];
let selectedIndex = -1;
let editingId = null;
let pendingCustomIconPath = null;
let pendingAutoIcon = false;
let nameModalMode = null; // 'newIsland' | 'rename'
let nameModalTargetId = null;
let nameModalTargetKind = null;
let selectedIslandColor = null;
let moveModalItemId = null;
let moveModalBulkIds = null;

// Multi-select (drag-select rectangle + Ctrl/Shift-click), independent of
// selectedIndex above (which is single-item keyboard-arrow focus).
let selectedIds = new Set();

// ---------- helpers ----------

function clearSelection() {
  if (selectedIds.size === 0) return;
  selectedIds.clear();
  updateSelectionVisuals();
}

function toggleSelected(id) {
  if (selectedIds.has(id)) selectedIds.delete(id);
  else selectedIds.add(id);
  updateSelectionVisuals();
}

function updateSelectionVisuals() {
  for (const el of canvas.querySelectorAll('[data-id]')) {
    el.classList.toggle('multi-selected', selectedIds.has(el.dataset.id));
  }
}

function folderSVG(color) {
  const c = color || ISLAND_COLORS[0];
  return `<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path fill="${c}" d="M3 6.5A2.5 2.5 0 0 1 5.5 4h4l2 2h7A2.5 2.5 0 0 1 21 8.5v9A2.5 2.5 0 0 1 18.5 20h-13A2.5 2.5 0 0 1 3 17.5v-11Z"/></svg>`;
}

// Windows won't hand over its real recycle-bin icon through any API path
// available (see the comment on the "special" field where it's set) - this
// is a stand-in instead, loaded from icons/recycle-bin.svg like the rest of
// the icon set (see ICON_FILES below) but colored to match regular text
// rather than the accent color, since it sits among plain app icons on the
// grid rather than in the accent-tinted taskbar.
function recycleBinSVG() {
  return ICONS.recycleBin;
}

// ---------- taskbar icon set ----------
// Small solid-glyph icons for the taskbar's system-tray-style buttons, kept
// as actual files under renderer/icons/ so they're easy to find and replace
// - edit a file's markup (or drop in a whole new one, same viewBox) and
// reload the app. Loaded via synchronous XHR (not <img>, which would bake
// in whatever colors the file itself specifies) so each one lands as real
// inline DOM and its `fill="currentColor"`/`stroke="currentColor"` picks up
// each button's own `color`, which every taskbar icon button ties to
// var(--accent) via CSS - that's what keeps them all in sync with the
// user's chosen accent color.
const ICON_FILES = {
  volumeHigh: 'volume-high',
  volumeMedium: 'volume-medium',
  volumeLow: 'volume-low',
  volumeMuted: 'volume-muted',
  eject: 'eject',
  recent: 'recent',
  trash: 'trash',
  settings: 'settings',
  usb: 'usb',
  network: 'network',
  networkOff: 'network-off',
  bluetooth: 'bluetooth',
  power: 'power',
  fullscreenEnter: 'fullscreen-enter',
  fullscreenExit: 'fullscreen-exit',
  recycleBin: 'recycle-bin',
};

function loadIconFile(fileName) {
  try {
    const xhr = new XMLHttpRequest();
    xhr.open('GET', `icons/${fileName}.svg`, false); // synchronous - these are tiny local files
    xhr.send(null);
    if (xhr.status === 200 || xhr.status === 0) return xhr.responseText;
  } catch {}
  return '';
}

const ICONS = {};
for (const [key, fileName] of Object.entries(ICON_FILES)) {
  ICONS[key] = loadIconFile(fileName);
}

settingsBtn.innerHTML = ICONS.settings;
recentBtn.innerHTML = ICONS.recent;
usbBtn.innerHTML = ICONS.usb;
bluetoothBtn.innerHTML = ICONS.bluetooth;
powerBtn.innerHTML = ICONS.power;

function iconNode(item, extraClass) {
  let el;
  if (item.kind === 'island') {
    el = document.createElement('div');
    el.className = 'folder-icon';
    el.innerHTML = folderSVG(item.color);
  } else if (item.special === 'recycleBin' && !item.iconIsCustom) {
    el = document.createElement('div');
    el.className = 'folder-icon';
    el.style.color = 'var(--text)';
    el.innerHTML = recycleBinSVG();
  } else {
    el = document.createElement('img');
    el.alt = '';
    el.src = item.iconDataUrl || PLACEHOLDER_ICON;
  }
  if (extraClass) el.classList.add(extraClass);
  // Images (and links) are natively draggable by default, which competes
  // with the tile's own drag handling and confuses it for an external file
  // drag. The tile div is the only thing that should drive the drag.
  el.draggable = false;
  return el;
}

function getAncestors(islandId) {
  const chain = [];
  let cur = islandId;
  while (cur) {
    const f = apps.find((a) => a.id === cur && a.kind === 'island');
    if (!f) break;
    chain.unshift(f);
    cur = f.parentId || null;
  }
  return chain;
}

function getPathLabel(islandId) {
  const chain = getAncestors(islandId);
  return chain.length ? chain.map((f) => f.name).join(' / ') : 'Home';
}

function descendantIslandIds(islandId) {
  const ids = new Set([islandId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const a of apps) {
      if (a.kind === 'island' && ids.has(a.parentId) && !ids.has(a.id)) {
        ids.add(a.id);
        grew = true;
      }
    }
  }
  return ids;
}

async function refreshFromServer(promise) {
  apps = await promise;
  // If the island we're standing in just got removed, bump back to Home
  // instead of showing a dead end.
  if (currentFolderId && !apps.some((a) => a.id === currentFolderId && a.kind === 'island')) {
    currentFolderId = null;
  }
  render();
  renderPinned();
}

// ---------- grid / breadcrumb ----------

function renderBreadcrumb() {
  breadcrumbEl.innerHTML = '';
  const chain = getAncestors(currentFolderId);

  const homeBtn = document.createElement('button');
  homeBtn.type = 'button';
  homeBtn.className = 'crumb' + (chain.length === 0 ? ' current' : '');
  homeBtn.textContent = 'Home';
  homeBtn.addEventListener('click', () => {
    currentFolderId = null;
    render();
  });
  addDropTarget(homeBtn, null);
  breadcrumbEl.appendChild(homeBtn);

  chain.forEach((f, i) => {
    const sep = document.createElement('span');
    sep.className = 'sep';
    sep.textContent = '›';
    breadcrumbEl.appendChild(sep);

    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'crumb' + (i === chain.length - 1 ? ' current' : '');
    btn.textContent = f.name;
    btn.addEventListener('click', () => {
      currentFolderId = f.id;
      render();
    });
    if (i === chain.length - 1) {
      btn.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        showItemContextMenu(e.clientX, e.clientY, f);
      });
    }
    addDropTarget(btn, f.id);
    breadcrumbEl.appendChild(btn);
  });
}

function isInternalDrag(e) {
  return e.dataTransfer.types.includes('application/x-launchpad-id');
}

// Lets an item be dropped onto `el` to move it into island `targetParentId`
// (null = Home/top level). Used for island tiles/cards and breadcrumb crumbs.
function addDropTarget(el, targetParentId) {
  el.addEventListener('dragover', (e) => {
    if (!isInternalDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    el.classList.add('drag-over');
  });
  el.addEventListener('dragleave', () => el.classList.remove('drag-over'));
  el.addEventListener('drop', async (e) => {
    if (!isInternalDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    el.classList.remove('drag-over');

    const groupData = e.dataTransfer.getData('application/x-launchpad-ids');
    if (groupData) {
      const ids = JSON.parse(groupData).filter((gid) => gid !== targetParentId);
      for (const gid of ids) await window.launcherAPI.moveItem(gid, targetParentId);
      if (ids.length) await refreshFromServer(window.launcherAPI.listApps());
      return;
    }
    const draggedId = e.dataTransfer.getData('application/x-launchpad-id');
    if (draggedId && draggedId !== targetParentId) {
      await refreshFromServer(window.launcherAPI.moveItem(draggedId, targetParentId));
    }
  });
}

function buildTile(item) {
  const tile = document.createElement('div');
  tile.className = 'tile';
  if (item.missing) tile.classList.add('missing');
  if (selectedIds.has(item.id)) tile.classList.add('multi-selected');
  tile.title = item.missing ? `${item.name}\n⚠ Target not found:\n${item.target}` : item.name;
  tile.draggable = true;
  tile.dataset.id = item.id;

  tile.appendChild(iconNode(item, 'tile-icon'));

  const label = document.createElement('div');
  label.className = 'tile-name';
  label.textContent = item.name;
  tile.appendChild(label);

  const editBtn = document.createElement('button');
  editBtn.className = 'edit-btn';
  editBtn.type = 'button';
  editBtn.textContent = '✎';
  editBtn.title = 'Edit';
  editBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    if (item.kind === 'island') openRenameModal(item);
    else openEditModal(item);
  });
  tile.appendChild(editBtn);

  tile.addEventListener('click', (e) => {
    if (e.shiftKey || e.ctrlKey || e.metaKey) {
      e.stopPropagation();
      toggleSelected(item.id);
      return;
    }
    if (selectedIds.size > 0) {
      // A plain click while a selection is active just clears it, rather
      // than also launching whatever happened to be clicked on.
      clearSelection();
      return;
    }
    if (item.kind === 'island') {
      currentFolderId = item.id;
      render();
    } else {
      launchApp(item);
    }
  });

  tile.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    e.stopPropagation();
    showItemContextMenu(e.clientX, e.clientY, item);
  });

  tile.addEventListener('dragstart', (e) => setDragData(e, item.id));

  if (item.kind === 'island') addDropTarget(tile, item.id);

  return tile;
}

// If the dragged item is part of a multi-selection, carry the whole
// selection along as a second data type - drop handlers check for this
// first and, when present, move/reposition every id in it together.
function setDragData(e, id) {
  if (selectedIds.has(id) && selectedIds.size > 1) {
    e.dataTransfer.setData('application/x-launchpad-ids', JSON.stringify([...selectedIds]));
  }
  e.dataTransfer.setData('application/x-launchpad-id', id);
  e.dataTransfer.effectAllowed = 'move';
}

// Expanded island: a live mini-card showing name/color and several of its
// apps as directly-clickable icons, instead of a single collapsed icon.
// Sized to its own w/h footprint - icons scale with it, and a resize handle
// in the corner lets the user drag that footprint bigger/smaller.
function buildIslandCard(item) {
  const fp = footprintOf(item);

  const card = document.createElement('div');
  card.className = 'island-card';
  if (selectedIds.has(item.id)) card.classList.add('multi-selected');
  card.title = item.name;
  card.draggable = true;
  card.dataset.id = item.id;
  card.style.setProperty('--island-color', item.color || ISLAND_COLORS[0]);
  const cardW = fp.w * CELL - CARD_MARGIN;
  const cardH = fp.h * CELL - CARD_MARGIN;
  card.style.width = `${cardW}px`;
  card.style.height = `${cardH}px`;
  const iconSize = Math.max(28, Math.min(64, Math.round((fp.h * CELL) / 3)));
  card.style.setProperty('--icon-size', `${iconSize}px`);

  // How many icons actually fit, measured against the real body area -
  // matches ISLAND_CARD_HEADER_H/BODY_PAD/BODY_GAP in styles.css. Rows get
  // taller when names are shown, since each mini-icon then has a label
  // under it.
  const bodyW = cardW - ISLAND_BODY_PAD;
  const bodyH = cardH - ISLAND_HEADER_H - ISLAND_BODY_PAD;
  const rowH = iconSize + ISLAND_BODY_GAP + (item.showNames ? ISLAND_NAME_LABEL_H : 0);
  const cols = Math.max(1, Math.floor((bodyW + ISLAND_BODY_GAP) / (iconSize + ISLAND_BODY_GAP)));
  const rows = Math.max(1, Math.floor((bodyH + ISLAND_BODY_GAP) / rowH));
  const capacity = cols * rows;

  const header = document.createElement('div');
  header.className = 'island-card-header';

  const nameEl = document.createElement('span');
  nameEl.className = 'island-card-name';
  nameEl.textContent = item.name;
  header.appendChild(nameEl);

  const collapseBtn = document.createElement('button');
  collapseBtn.type = 'button';
  collapseBtn.className = 'island-collapse-btn';
  collapseBtn.title = 'Minimize';
  collapseBtn.textContent = '–';
  collapseBtn.addEventListener('click', async (e) => {
    e.stopPropagation();
    await refreshFromServer(window.launcherAPI.setCollapsed(item.id, true));
  });
  header.appendChild(collapseBtn);

  header.addEventListener('click', (e) => {
    e.stopPropagation();
    if (e.shiftKey || e.ctrlKey || e.metaKey) {
      toggleSelected(item.id);
      return;
    }
    if (selectedIds.size > 0) {
      clearSelection();
      return;
    }
    currentFolderId = item.id;
    render();
  });
  card.appendChild(header);

  const contents = apps.filter((a) => (a.parentId || null) === item.id).sort((a, b) => a.name.localeCompare(b.name));

  const body = document.createElement('div');
  body.className = 'island-card-body';

  if (contents.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'island-card-empty';
    empty.textContent = 'Empty — drag apps here';
    body.appendChild(empty);
  } else {
    // If everything fits, show it all; otherwise leave one slot free for
    // the "+N more" tile so the total never exceeds what actually fits.
    const shown = contents.length <= capacity ? contents : contents.slice(0, Math.max(1, capacity - 1));
    for (const child of shown) {
      const mini = document.createElement('div');
      mini.className = 'island-mini-icon' + (item.showNames ? ' with-name' : '');
      mini.title = child.name;
      mini.draggable = true;
      mini.appendChild(iconNode(child));
      if (item.showNames) {
        const nameLabel = document.createElement('span');
        nameLabel.className = 'mini-icon-name';
        nameLabel.textContent = child.name;
        mini.appendChild(nameLabel);
      }
      mini.addEventListener('click', (e) => {
        e.stopPropagation();
        if (child.kind === 'island') {
          currentFolderId = child.id;
          render();
        } else {
          launchApp(child);
        }
      });
      mini.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        showItemContextMenu(e.clientX, e.clientY, child);
      });
      mini.addEventListener('dragstart', (e) => {
        e.stopPropagation(); // don't also start dragging the card itself
        setDragData(e, child.id);
      });
      if (child.kind === 'island') addDropTarget(mini, child.id);
      body.appendChild(mini);
    }
    if (contents.length > shown.length) {
      const more = document.createElement('div');
      more.className = 'island-mini-icon island-more';
      more.textContent = `+${contents.length - shown.length}`;
      more.title = 'See all';
      more.addEventListener('click', (e) => {
        e.stopPropagation();
        currentFolderId = item.id;
        render();
      });
      body.appendChild(more);
    }
  }
  card.appendChild(body);

  card.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    e.stopPropagation();
    showItemContextMenu(e.clientX, e.clientY, item);
  });
  card.addEventListener('dragstart', (e) => setDragData(e, item.id));
  addDropTarget(card, item.id);
  attachResizeHandle(card, item, fp);

  return card;
}

// Bottom-right corner handle: drag to resize the island's footprint (in
// whole grid cells, snapped) rather than the HTML5 drag-and-drop used for
// moving/repositioning items.
function attachResizeHandle(card, item, startFp) {
  const handle = document.createElement('div');
  handle.className = 'island-resize-handle';
  handle.title = 'Resize';
  handle.draggable = false;

  handle.addEventListener('mousedown', (e) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startY = e.clientY;
    let newW = startFp.w;
    let newH = startFp.h;
    card.classList.add('resizing');

    function onMove(ev) {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      // Follow the cursor continuously while dragging - only the value
      // actually committed on release snaps to the grid. Snapping the
      // live preview to CELL-sized steps is what made it visibly jump.
      const rawW = Math.max(2 * CELL - CARD_MARGIN, startFp.w * CELL - CARD_MARGIN + dx);
      const rawH = Math.max(2 * CELL - CARD_MARGIN, startFp.h * CELL - CARD_MARGIN + dy);
      card.style.width = `${rawW}px`;
      card.style.height = `${rawH}px`;
      newW = Math.max(2, Math.min(8, Math.round((rawW + CARD_MARGIN) / CELL)));
      newH = Math.max(2, Math.min(6, Math.round((rawH + CARD_MARGIN) / CELL)));
    }
    function onUp() {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      card.classList.remove('resizing');
      if (newW !== startFp.w || newH !== startFp.h) {
        refreshFromServer(window.launcherAPI.setSize(item.id, newW, newH));
      }
    }
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  });

  card.appendChild(handle);
}

function buildItemElement(item) {
  return item.kind === 'island' && item.collapsed === false ? buildIslandCard(item) : buildTile(item);
}

// Absolute-positioned, snap-to-grid canvas - used at every level (Home, and
// inside each island). Everything gets a saved gridX/gridY cell, scoped to
// whatever parent it's currently in.
function renderCanvas(items) {
  const sorted = [...items].sort((a, b) => {
    const ay = typeof a.gridY === 'number' ? a.gridY : 0;
    const by = typeof b.gridY === 'number' ? b.gridY : 0;
    if (ay !== by) return ay - by;
    const ax = typeof a.gridX === 'number' ? a.gridX : 0;
    const bx = typeof b.gridX === 'number' ? b.gridX : 0;
    return ax - bx;
  });

  let maxRow = 0;
  let maxCol = 0;
  for (const item of sorted) {
    const el = buildItemElement(item);
    const fp = footprintOf(item);
    const gx = typeof item.gridX === 'number' ? item.gridX : 0;
    const gy = typeof item.gridY === 'number' ? item.gridY : 0;
    el.style.left = `${gx * CELL}px`;
    el.style.top = `${gy * CELL}px`;
    maxRow = Math.max(maxRow, gy + fp.h);
    maxCol = Math.max(maxCol, gx + fp.w);
    canvas.appendChild(el);
  }
  // Sized on the CONTENT wrapper, not the scroll viewport - see the comment
  // on .grid/.canvas in styles.css for why that split matters. Also floored
  // to the viewport's own available content height: a block with only
  // absolutely-positioned children has zero natural height, so with few
  // icons the canvas would otherwise end short of the visible .grid area,
  // leaving a dead zone below the icons where mousedown never reaches canvas
  // and drag-select silently does nothing. Subtracting .grid's own padding
  // is what keeps this floor from itself creating ~40px of overflow (and a
  // bogus scrollbar) when there are zero or few apps.
  const gridStyle = getComputedStyle(grid);
  const gridVPad = parseFloat(gridStyle.paddingTop) + parseFloat(gridStyle.paddingBottom);
  const availableHeight = grid.clientHeight - gridVPad;
  canvas.style.minHeight = `${Math.max(maxRow * CELL + 24, availableHeight)}px`;
  canvas.style.minWidth = `${maxCol * CELL + 24}px`;
  return sorted;
}

function render() {
  // Clear multi-select exactly when the viewed folder actually changes
  // (not on every data refresh - lots of call sites navigate, this catches
  // all of them centrally instead of needing a clearSelection() at each one).
  if (currentFolderId !== lastRenderedFolderId) {
    selectedIds.clear();
    lastRenderedFolderId = currentFolderId;
  }

  renderBreadcrumb();

  const items = apps.filter((a) => (a.parentId || null) === currentFolderId);

  canvas.innerHTML = '';
  canvas.style.minHeight = '';
  canvas.style.minWidth = '';
  currentItems = renderCanvas(items);
  selectedIndex = -1;

  const isEmpty = items.length === 0;
  emptyState.classList.toggle('hidden', !isEmpty);
  grid.classList.toggle('hidden', isEmpty);

  if (isEmpty) {
    emptyTitle.textContent = currentFolderId ? 'This island is empty.' : 'No apps yet.';
    emptySubtitle.innerHTML = currentFolderId
      ? 'Right-click to add something, or click <strong>+ Add App</strong>.'
      : 'Click <strong>+ Add App</strong> to add your first one, or right-click for an island.';
  }
}

// Reposition-on-drop: rearranges within whatever level is currently open
// (Home or an island's own contents). Only fires when the drop didn't land
// on an island's own drop target (those stopPropagation, so this never
// fires for a containment move).
grid.addEventListener('dragover', (e) => {
  if (!isInternalDrag(e)) return;
  e.preventDefault();
});
grid.addEventListener('drop', async (e) => {
  if (!isInternalDrag(e)) return;
  e.preventDefault();
  const draggedId = e.dataTransfer.getData('application/x-launchpad-id');
  if (!draggedId) return;
  const rect = grid.getBoundingClientRect();
  const relX = e.clientX - rect.left + grid.scrollLeft;
  const relY = e.clientY - rect.top + grid.scrollTop;
  const gridX = Math.max(0, Math.round(relX / CELL - 0.5));
  const gridY = Math.max(0, Math.round(relY / CELL - 0.5));

  const groupData = e.dataTransfer.getData('application/x-launchpad-ids');
  if (groupData) {
    // Move the whole selection together: every id shifts by the same
    // delta the dragged item itself moved.
    const draggedItem = apps.find((a) => a.id === draggedId);
    const dx = gridX - (draggedItem && typeof draggedItem.gridX === 'number' ? draggedItem.gridX : gridX);
    const dy = gridY - (draggedItem && typeof draggedItem.gridY === 'number' ? draggedItem.gridY : gridY);
    const ids = JSON.parse(groupData);
    const updates = ids.map((id) => {
      const it = apps.find((a) => a.id === id);
      const ix = it && typeof it.gridX === 'number' ? it.gridX : 0;
      const iy = it && typeof it.gridY === 'number' ? it.gridY : 0;
      return { id, gridX: ix + dx, gridY: iy + dy };
    });
    await refreshFromServer(window.launcherAPI.setPositions(updates));
    return;
  }
  await refreshFromServer(window.launcherAPI.setPosition(draggedId, gridX, gridY));
});

// ---------- drag-select (marquee) ----------

grid.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return; // left button only
  // Must start on empty space, not a tile/card. Listening on .grid (the
  // scroll viewport) rather than .canvas (the content box) matters because
  // .canvas can end shorter or narrower than the visible viewport - see the
  // comment in renderCanvas(). Checking closest('[data-id]') instead of an
  // exact-element match keeps this working regardless of which of the two
  // empty-space elements the click actually landed on.
  if (e.target.closest('[data-id]')) return;
  e.preventDefault();

  const additive = e.shiftKey || e.ctrlKey || e.metaKey;
  const baseSelection = additive ? new Set(selectedIds) : new Set();
  if (!additive) clearSelection();

  const canvasRect = canvas.getBoundingClientRect();
  const startX = e.clientX - canvasRect.left;
  const startY = e.clientY - canvasRect.top;

  // Snapshot every tile's box once, up front - positions can't change during
  // a marquee drag, so re-reading offsetLeft/Top from the live DOM on every
  // mousemove (right after writing the marquee's own style) forced a
  // synchronous layout flush per event. That read-after-write thrashing is
  // what made the marquee feel laggy and made fast drags miss/skip tiles.
  const candidates = Array.from(canvas.querySelectorAll('[data-id]')).map((el) => ({
    id: el.dataset.id,
    left: el.offsetLeft,
    top: el.offsetTop,
    right: el.offsetLeft + el.offsetWidth,
    bottom: el.offsetTop + el.offsetHeight,
  }));

  const marquee = document.createElement('div');
  marquee.className = 'marquee';
  canvas.appendChild(marquee);
  let moved = false;
  let pendingEvent = null;
  let rafId = null;

  function applyMove(ev) {
    const curRect = canvas.getBoundingClientRect();
    const curX = ev.clientX - curRect.left;
    const curY = ev.clientY - curRect.top;
    const left = Math.min(startX, curX);
    const top = Math.min(startY, curY);
    const w = Math.abs(curX - startX);
    const h = Math.abs(curY - startY);
    if (w > 3 || h > 3) moved = true;
    marquee.style.left = `${left}px`;
    marquee.style.top = `${top}px`;
    marquee.style.width = `${w}px`;
    marquee.style.height = `${h}px`;

    const right = left + w;
    const bottom = top + h;
    const next = new Set(baseSelection);
    for (const c of candidates) {
      if (c.left < right && c.right > left && c.top < bottom && c.bottom > top) next.add(c.id);
    }
    selectedIds = next;
    updateSelectionVisuals();
  }

  function onMove(ev) {
    pendingEvent = ev;
    if (rafId !== null) return;
    rafId = requestAnimationFrame(() => {
      rafId = null;
      applyMove(pendingEvent);
    });
  }

  function onUp() {
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    if (rafId !== null) cancelAnimationFrame(rafId);
    marquee.remove();
    if (!moved && !additive) clearSelection(); // plain click on empty space
  }

  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
});

async function launchApp(item) {
  try {
    await window.launcherAPI.launchApp(item.id);
  } catch (err) {
    await showAlert(`Couldn't launch "${item.name}":\n${err.message || err}`);
  }
}

// ---------- keyboard navigation ----------

function setSelected(i) {
  const tiles = Array.from(canvas.children);
  tiles.forEach((t) => t.classList.remove('selected'));
  selectedIndex = i;
  if (i >= 0 && tiles[i]) {
    tiles[i].classList.add('selected');
    tiles[i].scrollIntoView({ block: 'nearest' });
  }
}

function moveSelection(dx, dy) {
  const tiles = Array.from(canvas.children);
  if (tiles.length === 0) return;
  if (selectedIndex === -1) {
    setSelected(0);
    return;
  }
  if (dx !== 0) {
    const next = Math.max(0, Math.min(tiles.length - 1, selectedIndex + dx));
    setSelected(next);
    return;
  }
  const cur = tiles[selectedIndex];
  const curTop = cur.offsetTop;
  const curLeft = cur.offsetLeft;
  let best = -1;
  let bestRowDist = Infinity;
  let bestColDist = Infinity;
  tiles.forEach((t, i) => {
    if (dy < 0 && t.offsetTop >= curTop) return;
    if (dy > 0 && t.offsetTop <= curTop) return;
    const rowDist = Math.abs(t.offsetTop - curTop);
    const colDist = Math.abs(t.offsetLeft - curLeft);
    if (rowDist < bestRowDist || (rowDist === bestRowDist && colDist < bestColDist)) {
      best = i;
      bestRowDist = rowDist;
      bestColDist = colDist;
    }
  });
  if (best !== -1) setSelected(best);
}

function activateSelected() {
  const item = currentItems[selectedIndex];
  if (!item) return;
  if (item.kind === 'island') {
    currentFolderId = item.id;
    render();
  } else {
    launchApp(item);
  }
}

function anyModalOpen() {
  return (
    !modalOverlay.classList.contains('hidden') ||
    !nameModalOverlay.classList.contains('hidden') ||
    !moveModalOverlay.classList.contains('hidden') ||
    !settingsModalOverlay.classList.contains('hidden') ||
    !dialogOverlay.classList.contains('hidden') ||
    !contextMenu.classList.contains('hidden')
  );
}

// Custom, non-blocking replacements for window.alert()/confirm(). Electron's
// native blocking dialogs on Windows can leave the renderer's keyboard focus
// desynced after they close - every field in the app stops receiving key
// events, and it doesn't self-heal even if the window is hidden and shown
// again since the state is stuck at the native widget level, not in JS.
let dialogResolve = null;

function showDialog(message, showCancel) {
  return new Promise((resolve) => {
    dialogMessage.textContent = message;
    dialogCancelBtn.classList.toggle('hidden', !showCancel);
    dialogOverlay.classList.remove('hidden');
    dialogResolve = resolve;
    dialogOkBtn.focus();
  });
}

function closeDialog(result) {
  if (dialogOverlay.classList.contains('hidden')) return;
  dialogOverlay.classList.add('hidden');
  const resolve = dialogResolve;
  dialogResolve = null;
  if (resolve) resolve(result);
}

function showAlert(message) {
  return showDialog(message, false);
}

function showConfirm(message) {
  return showDialog(message, true);
}

dialogOkBtn.addEventListener('click', () => closeDialog(true));
dialogCancelBtn.addEventListener('click', () => closeDialog(false));
dialogOverlay.addEventListener('click', (e) => {
  if (e.target === dialogOverlay) closeDialog(false);
});

document.addEventListener('keydown', (e) => {
  if (anyModalOpen()) return;
  const activeTag = document.activeElement ? document.activeElement.tagName : '';
  const typingInField = activeTag === 'INPUT' || activeTag === 'TEXTAREA' || activeTag === 'SELECT';

  if (typingInField) return;

  if (e.key === 'ArrowRight') {
    e.preventDefault();
    moveSelection(1, 0);
  } else if (e.key === 'ArrowLeft') {
    e.preventDefault();
    moveSelection(-1, 0);
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    moveSelection(0, 1);
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    moveSelection(0, -1);
  } else if (e.key === 'Enter') {
    if (selectedIndex >= 0) {
      e.preventDefault();
      activateSelected();
    }
  } else if (/^[a-zA-Z0-9]$/.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey) {
    taskbarSearch.focus();
    taskbarSearch.value += e.key;
    taskbarSearch.dispatchEvent(new Event('input'));
  }
});

// ---------- external file drop (drag from Explorer to add) ----------

document.body.addEventListener('dragover', (e) => {
  if (isInternalDrag(e) || !e.dataTransfer.types.includes('Files')) return;
  e.preventDefault();
  document.body.classList.add('file-drag-over');
});
document.body.addEventListener('dragleave', (e) => {
  if (e.target === document.body) document.body.classList.remove('file-drag-over');
});
document.body.addEventListener('drop', async (e) => {
  if (isInternalDrag(e) || !e.dataTransfer.types.includes('Files')) return;
  e.preventDefault();
  document.body.classList.remove('file-drag-over');
  const files = Array.from(e.dataTransfer.files || []);
  for (const file of files) {
    const filePath = window.launcherAPI.getPathForFile(file);
    if (filePath) await quickAddFromPath(filePath);
  }
});

async function quickAddFromPath(filePath) {
  const dot = filePath.lastIndexOf('.');
  const slash = Math.max(filePath.lastIndexOf('\\'), filePath.lastIndexOf('/'));
  const ext = dot > slash ? filePath.slice(dot + 1).toLowerCase() : '';
  let type = 'path';
  let interpreter = '';
  if (ext === 'py') {
    type = 'script';
    interpreter = 'python';
  } else if (ext === 'js' || ext === 'mjs') {
    type = 'script';
    interpreter = 'node';
  }
  const base = filePath.slice(slash + 1);
  const name = ext ? base.slice(0, -(ext.length + 1)) : base;

  await refreshFromServer(
    window.launcherAPI.addApp({ name, type, target: filePath, interpreter, parentId: currentFolderId })
  );
}

// ---------- taskbar: pinned row ----------

// Tracks the currently-displayed pin order (for the row-level drop handler
// below) and which gap a drag is currently hovering over (for the insertion
// line and the eventual drop).
let currentPinnedOrder = [];
let pendingPinInsertIndex = -1;
let openWindowsCount = 0;

// Shared by renderPinned() and refreshOpenWindows() - the hint text is only
// useful when the whole left zone would otherwise be empty; with anything
// pinned OR any window already open, showing it just crowds that limited
// column for no benefit.
function updatePinnedEmptyVisibility() {
  const pinnedCount = apps.filter((a) => a.pinned).length;
  pinnedEmpty.classList.toggle('hidden', pinnedCount > 0 || openWindowsCount > 0);
}

function renderPinned() {
  const pinned = apps.filter((a) => a.pinned).sort((a, b) => (a.pinOrder ?? Infinity) - (b.pinOrder ?? Infinity));
  currentPinnedOrder = pinned.map((p) => p.id);
  Array.from(pinnedRow.querySelectorAll('.pin-tile')).forEach((el) => el.remove());
  updatePinnedEmptyVisibility();

  for (const item of pinned) {
    const el = document.createElement('div');
    el.className = 'pin-tile';
    el.title = item.name;
    el.draggable = true;
    el.appendChild(iconNode(item));
    el.addEventListener('click', () => {
      if (item.kind === 'island') {
        currentFolderId = item.id;
        render();
      } else {
        launchApp(item);
      }
    });
    el.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      e.stopPropagation();
      showItemContextMenu(e.clientX, e.clientY, item);
    });
    el.addEventListener('dragstart', (e) => {
      e.dataTransfer.setData('application/x-launchpad-pin-id', item.id);
      e.dataTransfer.effectAllowed = 'move';
    });
    pinnedRow.appendChild(el);
  }
}

// Row-level (not per-tile) so a single insertion line can be drawn between
// whichever two tiles the cursor is currently between, instead of just
// outlining whatever tile happens to be under the cursor.
pinnedRow.addEventListener('dragover', (e) => {
  if (!e.dataTransfer.types.includes('application/x-launchpad-pin-id')) return;
  e.preventDefault();

  const tiles = Array.from(pinnedRow.querySelectorAll('.pin-tile'));
  const rowRect = pinnedRow.getBoundingClientRect();
  let insertIndex = tiles.length;
  let lineX = tiles.length ? tiles[tiles.length - 1].getBoundingClientRect().right - rowRect.left : 0;

  for (let i = 0; i < tiles.length; i++) {
    const r = tiles[i].getBoundingClientRect();
    if (e.clientX < r.left + r.width / 2) {
      insertIndex = i;
      lineX = r.left - rowRect.left;
      break;
    }
  }

  pendingPinInsertIndex = insertIndex;
  pinInsertLine.style.left = `${lineX + pinnedRow.scrollLeft}px`;
  pinInsertLine.classList.remove('hidden');
});

pinnedRow.addEventListener('dragleave', (e) => {
  if (!pinnedRow.contains(e.relatedTarget)) pinInsertLine.classList.add('hidden');
});

pinnedRow.addEventListener('drop', async (e) => {
  if (!e.dataTransfer.types.includes('application/x-launchpad-pin-id')) return;
  e.preventDefault();
  pinInsertLine.classList.add('hidden');

  const draggedId = e.dataTransfer.getData('application/x-launchpad-pin-id');
  if (!draggedId) return;
  const order = [...currentPinnedOrder];
  const fromIndex = order.indexOf(draggedId);
  if (fromIndex === -1) return;

  order.splice(fromIndex, 1);
  let insertAt = pendingPinInsertIndex;
  if (fromIndex < insertAt) insertAt -= 1; // removing the dragged item shifted everything after it left
  order.splice(Math.max(0, insertAt), 0, draggedId);
  await refreshFromServer(window.launcherAPI.setPinOrder(order));
});

// ---------- taskbar: search + recent popouts ----------

// Shared row builder for both the search-results and Recent popouts.
function renderResultRows(container, items, emptyText, onSelect) {
  container.innerHTML = '';
  if (items.length === 0) {
    const div = document.createElement('div');
    div.className = 'search-empty';
    div.textContent = emptyText;
    container.appendChild(div);
    return;
  }
  for (const item of items) {
    const row = document.createElement('div');
    row.className = 'search-result';
    row.appendChild(iconNode(item));

    const info = document.createElement('div');
    const nameEl = document.createElement('div');
    nameEl.className = 'sr-name';
    nameEl.textContent = item.name;
    const pathEl = document.createElement('div');
    pathEl.className = 'sr-path';
    pathEl.textContent = getPathLabel(item.parentId || null);
    info.appendChild(nameEl);
    info.appendChild(pathEl);
    row.appendChild(info);

    row.addEventListener('click', () => onSelect(item));
    container.appendChild(row);
  }
}

async function selectSearchResult(item) {
  searchResults.classList.add('hidden');
  taskbarSearch.value = '';
  taskbarSearch.blur();
  if (item.kind === 'island') {
    currentFolderId = item.id;
    render();
  } else {
    await launchApp(item);
  }
}

taskbarSearch.addEventListener('input', () => {
  const q = taskbarSearch.value.trim().toLowerCase();
  if (!q) {
    searchResults.classList.add('hidden');
    return;
  }
  closeAllTaskbarPopouts('search');
  const matches = apps.filter((a) => a.name.toLowerCase().includes(q)).slice(0, 20);
  renderResultRows(searchResults, matches, 'No matches.', selectSearchResult);
  searchResults.classList.remove('hidden');
});

taskbarSearch.addEventListener('focus', () => {
  if (taskbarSearch.value.trim()) {
    closeAllTaskbarPopouts('search');
    searchResults.classList.remove('hidden');
  }
});

// Opening any one taskbar popout closes every other one, so at most a
// single popout is ever showing at once - each button's click handler calls
// this (with its own popout named in `except`) before deciding whether to
// open or toggle-close its own.
function closeAllTaskbarPopouts(except) {
  if (except !== 'search') searchResults.classList.add('hidden');
  if (except !== 'recent') recentPopout.classList.add('hidden');
  if (except !== 'volume') closeVolumePopout();
  if (except !== 'usb') closeUsbPopout();
  if (except !== 'bluetooth') closeBluetoothPopout();
  if (except !== 'network') closeNetworkPopout();
  if (except !== 'power') closePowerPopout();
}

recentBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  const opening = recentPopout.classList.contains('hidden');
  closeAllTaskbarPopouts('recent');
  recentPopout.classList.toggle('hidden', !opening);
  if (!opening) return;

  const recent = apps
    .filter((a) => a.kind === 'app' && a.lastLaunched)
    .sort((a, b) => b.lastLaunched - a.lastLaunched)
    .slice(0, 8);
  renderResultRows(recentPopout, recent, 'Nothing launched yet.', async (item) => {
    recentPopout.classList.add('hidden');
    await launchApp(item);
  });
});

// ---------- clock ----------

function updateClock() {
  const now = new Date();
  clockTime.textContent = now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  clockDate.textContent = now.toLocaleDateString();
}
updateClock();
setInterval(updateClock, 1000);

// ---------- network status ----------

function updateNetworkStatus() {
  const online = navigator.onLine;
  networkBtn.innerHTML = online ? ICONS.network : ICONS.networkOff;
}
window.addEventListener('online', updateNetworkStatus);
window.addEventListener('offline', updateNetworkStatus);
updateNetworkStatus();

function closeNetworkPopout() {
  networkPopout.classList.add('hidden');
}

function networkRow(net) {
  const row = document.createElement('div');
  row.className = 'usb-row';

  const info = document.createElement('div');
  info.className = 'usb-info';
  const label = document.createElement('div');
  label.className = 'usb-label';
  label.textContent = net.ssid;
  const status = document.createElement('div');
  status.className = 'usb-size';
  status.textContent = net.connected ? 'Connected' : net.saved ? `${net.auth} · Saved` : net.auth;
  info.appendChild(label);
  info.appendChild(status);

  const action = document.createElement('button');
  action.type = 'button';
  action.className = 'wifi-action-btn';
  if (net.connected) {
    action.textContent = 'Disconnect';
    action.classList.add('danger');
    action.addEventListener('click', async () => {
      action.disabled = true;
      const res = await window.launcherAPI.disconnectWifi();
      if (!res.ok) await showAlert(`Couldn't disconnect:\n${res.error || 'Unknown error.'}`);
      await refreshNetworkList();
    });
  } else if (net.saved) {
    action.textContent = 'Connect';
    action.addEventListener('click', async () => {
      action.disabled = true;
      const res = await window.launcherAPI.connectWifi(net.ssid);
      if (!res.ok) await showAlert(`Couldn't connect to ${net.ssid}:\n${res.error || 'Unknown error.'}`);
      await refreshNetworkList();
    });
  } else {
    action.textContent = 'Connect';
    action.disabled = true;
    action.title = 'No saved password for this network yet - connect once via Windows Settings first';
  }

  row.appendChild(info);
  row.appendChild(action);
  return row;
}

async function refreshNetworkList() {
  const networks = await window.launcherAPI.listWifiNetworks();
  networkList.innerHTML = '';
  if (networks.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'usb-empty';
    empty.textContent = 'No Wi-Fi networks found.';
    networkList.appendChild(empty);
    return;
  }
  for (const net of networks) {
    networkList.appendChild(networkRow(net));
  }
}

networkBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  if (!networkPopout.classList.contains('hidden')) {
    closeNetworkPopout();
    return;
  }
  closeAllTaskbarPopouts('network');
  networkPopout.classList.remove('hidden');
  networkList.innerHTML = '<div class="usb-empty">Scanning…</div>';
  refreshNetworkList();
});

networkSettingsBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  window.launcherAPI.openNetworkSettings();
});

// ---------- volume ----------

let volumePollTimer = null;
let volumeSetDebounce = null;
let lastKnownMuted = false;
const sessionSetDebounce = new Map();

function volumeIconFor(pct, muted) {
  if (muted || pct === 0) return ICONS.volumeMuted;
  if (pct < 33) return ICONS.volumeLow;
  if (pct < 66) return ICONS.volumeMedium;
  return ICONS.volumeHigh;
}

function applyVolumeState({ volume: pct, muted }) {
  lastKnownMuted = !!muted;
  volumeSlider.value = pct;
  volumePct.textContent = `${pct}%`;
  volumeBtn.innerHTML = volumeIconFor(pct, muted);
  muteBtn.innerHTML = muted ? ICONS.volumeMuted : ICONS.volumeHigh;
}

async function refreshVolumeState() {
  applyVolumeState(await window.launcherAPI.getVolume());
}

refreshVolumeState();

async function refreshOutputDevices() {
  const devices = await window.launcherAPI.listAudioDevices();
  outputDeviceSelect.innerHTML = '';
  for (const dev of devices) {
    const opt = document.createElement('option');
    opt.value = dev.id;
    opt.textContent = dev.name;
    if (dev.isDefault) opt.selected = true;
    outputDeviceSelect.appendChild(opt);
  }
}

function sessionRow(session) {
  const row = document.createElement('div');
  row.className = 'session-row';

  const name = document.createElement('span');
  name.className = 'session-name';
  name.textContent = session.name;
  name.title = session.name;

  const mute = document.createElement('button');
  mute.type = 'button';
  mute.className = 'session-mute-btn';
  mute.innerHTML = session.muted ? ICONS.volumeMuted : ICONS.volumeHigh;
  mute.addEventListener('click', async () => {
    const nowMuted = !session.muted;
    mute.innerHTML = nowMuted ? ICONS.volumeMuted : ICONS.volumeHigh;
    await window.launcherAPI.setSessionMuted(session.pid, nowMuted);
  });

  const slider = document.createElement('input');
  slider.type = 'range';
  slider.min = '0';
  slider.max = '100';
  slider.step = '1';
  slider.className = 'session-slider';
  slider.value = session.volume;

  const pct = document.createElement('span');
  pct.className = 'session-pct';
  pct.textContent = `${session.volume}%`;

  slider.addEventListener('input', () => {
    const v = Number(slider.value);
    pct.textContent = `${v}%`;
    clearTimeout(sessionSetDebounce.get(session.pid));
    sessionSetDebounce.set(session.pid, setTimeout(() => window.launcherAPI.setSessionVolume(session.pid, v), 120));
  });

  row.appendChild(name);
  row.appendChild(mute);
  row.appendChild(slider);
  row.appendChild(pct);
  return row;
}

async function refreshSessionMixer() {
  const sessions = await window.launcherAPI.listAudioSessions();
  sessionMixerList.innerHTML = '';
  if (sessions.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'session-mixer-empty';
    empty.textContent = 'Nothing playing audio right now.';
    sessionMixerList.appendChild(empty);
    return;
  }
  for (const session of sessions) {
    sessionMixerList.appendChild(sessionRow(session));
  }
}

function closeVolumePopout() {
  volumePopout.classList.add('hidden');
  if (volumePollTimer) {
    clearInterval(volumePollTimer);
    volumePollTimer = null;
  }
}

volumeBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  if (!volumePopout.classList.contains('hidden')) {
    closeVolumePopout();
    return;
  }
  closeAllTaskbarPopouts('volume');
  volumePopout.classList.remove('hidden');
  if (!sessionMixerList.children.length) {
    sessionMixerList.innerHTML = '<div class="session-mixer-empty">Scanning…</div>';
  }
  refreshVolumeState();
  refreshOutputDevices();
  refreshSessionMixer();
  volumePollTimer = setInterval(() => {
    refreshVolumeState();
    refreshSessionMixer();
  }, 2000);
});

volumeSlider.addEventListener('input', () => {
  const pct = Number(volumeSlider.value);
  volumePct.textContent = `${pct}%`;
  volumeBtn.innerHTML = volumeIconFor(pct, lastKnownMuted);
  clearTimeout(volumeSetDebounce);
  volumeSetDebounce = setTimeout(() => window.launcherAPI.setVolume(pct), 120);
});

muteBtn.addEventListener('click', async (e) => {
  e.stopPropagation();
  await window.launcherAPI.setMuted(!lastKnownMuted);
  await refreshVolumeState();
});

outputDeviceSelect.addEventListener('change', async () => {
  await window.launcherAPI.setAudioDevice(outputDeviceSelect.value);
});

// ---------- USB devices ----------

function closeUsbPopout() {
  usbPopout.classList.add('hidden');
}

function formatBytes(bytes) {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`;
  return `${gb.toFixed(1)} GB`;
}

function usbRow(drive) {
  const row = document.createElement('div');
  row.className = 'usb-row';

  const info = document.createElement('div');
  info.className = 'usb-info';
  const label = document.createElement('div');
  label.className = 'usb-label';
  label.textContent = drive.label ? `${drive.label} (${drive.driveLetter})` : drive.driveLetter;
  const size = document.createElement('div');
  size.className = 'usb-size';
  size.textContent = `${formatBytes(drive.freeSpace)} free of ${formatBytes(drive.size)}`;
  info.appendChild(label);
  info.appendChild(size);

  const eject = document.createElement('button');
  eject.type = 'button';
  eject.className = 'usb-eject-btn';
  eject.title = 'Eject';
  eject.innerHTML = ICONS.eject;
  eject.addEventListener('click', async () => {
    eject.disabled = true;
    const res = await window.launcherAPI.ejectDrive(drive.driveLetter);
    if (!res.ok) {
      await showAlert(`Couldn't eject ${drive.driveLetter}:\n${res.error || 'Unknown error.'}`);
      eject.disabled = false;
      return;
    }
    await refreshUsbList();
  });

  row.appendChild(info);
  row.appendChild(eject);
  return row;
}

async function refreshUsbList() {
  const usbDrives = await window.launcherAPI.listUsbDrives();
  usbList.innerHTML = '';
  if (usbDrives.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'usb-empty';
    empty.textContent = 'No USB devices connected.';
    usbList.appendChild(empty);
    return;
  }
  for (const drive of usbDrives) {
    usbList.appendChild(usbRow(drive));
  }
}

usbBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  if (!usbPopout.classList.contains('hidden')) {
    closeUsbPopout();
    return;
  }
  closeAllTaskbarPopouts('usb');
  usbPopout.classList.remove('hidden');
  usbList.innerHTML = '<div class="usb-empty">Scanning…</div>';
  refreshUsbList();
});

// ---------- Bluetooth devices ----------

function closeBluetoothPopout() {
  bluetoothPopout.classList.add('hidden');
}

function bluetoothRow(device) {
  const row = document.createElement('div');
  row.className = 'usb-row';

  const info = document.createElement('div');
  info.className = 'usb-info';
  const label = document.createElement('div');
  label.className = 'usb-label';
  label.textContent = device.name;
  const status = document.createElement('div');
  status.className = 'usb-size';
  status.textContent = device.connected ? 'Connected' : 'Paired, not connected';
  info.appendChild(label);
  info.appendChild(status);

  const disconnect = document.createElement('button');
  disconnect.type = 'button';
  disconnect.className = 'usb-eject-btn';
  disconnect.title = 'Disconnect';
  disconnect.innerHTML = ICONS.eject;
  disconnect.disabled = !device.connected;
  disconnect.addEventListener('click', async () => {
    disconnect.disabled = true;
    const res = await window.launcherAPI.disconnectBluetoothDevice(device.instanceId);
    if (!res.ok) {
      await showAlert(`Couldn't disconnect ${device.name}:\n${res.error || 'Unknown error.'}`);
    }
    await refreshBluetoothList();
  });

  row.appendChild(info);
  row.appendChild(disconnect);
  return row;
}

async function refreshBluetoothList() {
  const devices = await window.launcherAPI.listBluetoothDevices();
  bluetoothList.innerHTML = '';
  if (devices.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'usb-empty';
    empty.textContent = 'No Bluetooth devices paired.';
    bluetoothList.appendChild(empty);
    return;
  }
  for (const device of devices) {
    bluetoothList.appendChild(bluetoothRow(device));
  }
}

bluetoothBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  if (!bluetoothPopout.classList.contains('hidden')) {
    closeBluetoothPopout();
    return;
  }
  closeAllTaskbarPopouts('bluetooth');
  bluetoothPopout.classList.remove('hidden');
  bluetoothList.innerHTML = '<div class="usb-empty">Scanning…</div>';
  refreshBluetoothList();
});

// ---------- power ----------

function closePowerPopout() {
  powerPopout.classList.add('hidden');
}

powerBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  if (!powerPopout.classList.contains('hidden')) {
    closePowerPopout();
    return;
  }
  closeAllTaskbarPopouts('power');
  powerPopout.classList.remove('hidden');
});

powerSleepBtn.addEventListener('click', async () => {
  closePowerPopout();
  await window.launcherAPI.sleep();
});

powerRestartBtn.addEventListener('click', async () => {
  closePowerPopout();
  if (!(await showConfirm('Restart this PC now? Unsaved work in other apps will be lost.'))) return;
  await window.launcherAPI.restart();
});

powerShutdownBtn.addEventListener('click', async () => {
  closePowerPopout();
  if (!(await showConfirm('Shut down this PC now? Unsaved work in other apps will be lost.'))) return;
  await window.launcherAPI.shutdown();
});

// ---------- fullscreen toggle ----------

function applyFullscreenIcon(isFullscreen) {
  fullscreenBtn.innerHTML = isFullscreen ? ICONS.fullscreenExit : ICONS.fullscreenEnter;
  fullscreenBtn.title = isFullscreen ? 'Exit fullscreen' : 'Enter fullscreen';
}

window.launcherAPI.isFullscreen().then(applyFullscreenIcon);
window.launcherAPI.onFullscreenChanged(applyFullscreenIcon);

fullscreenBtn.addEventListener('click', async () => {
  const res = await window.launcherAPI.toggleFullscreen();
  if (res && res.ok) applyFullscreenIcon(res.isFullscreen);
});

// ---------- open windows (Windows-taskbar-style running apps) ----------
// Separate from Launchpad's own "pinned" concept - this reflects whatever
// actually has a visible window open on the OS right now (any app, not just
// ones launched from here), refreshed on a poll since there's no push
// notification for "a window opened/closed" available to us.

function windowTile(win) {
  const el = document.createElement('div');
  el.className = 'window-tile';
  el.title = win.title;
  if (win.iconDataUrl) {
    const img = document.createElement('img');
    img.alt = '';
    img.src = win.iconDataUrl;
    el.appendChild(img);
  } else {
    const fallback = document.createElement('div');
    fallback.className = 'window-tile-fallback';
    fallback.textContent = (win.title || '?').trim().charAt(0).toUpperCase();
    el.appendChild(fallback);
  }
  el.addEventListener('click', () => window.launcherAPI.focusWindow(win.handle));
  return el;
}

async function refreshOpenWindows() {
  const list = await window.launcherAPI.listOpenWindows();
  openWindowsRow.innerHTML = '';
  for (const win of list) {
    openWindowsRow.appendChild(windowTile(win));
  }
  taskbarDivider.classList.toggle('hidden', list.length === 0);
  openWindowsCount = list.length;
  updatePinnedEmptyVisibility();
}

refreshOpenWindows();
setInterval(refreshOpenWindows, 3000);

// ---------- context menu ----------

function closeContextMenu() {
  contextMenu.classList.add('hidden');
}

function openContextMenu(x, y, menuItems) {
  contextMenu.innerHTML = '';
  for (const it of menuItems) {
    if (it.sep) {
      const sep = document.createElement('div');
      sep.className = 'menu-sep';
      contextMenu.appendChild(sep);
      continue;
    }
    const btn = document.createElement('button');
    btn.type = 'button';
    if (it.icon) {
      const iconSpan = document.createElement('span');
      iconSpan.className = 'menu-item-icon';
      iconSpan.innerHTML = it.icon;
      btn.appendChild(iconSpan);
    }
    btn.appendChild(document.createTextNode(it.label));
    if (it.danger) btn.classList.add('danger');
    btn.addEventListener('click', () => {
      closeContextMenu();
      it.action();
    });
    contextMenu.appendChild(btn);
  }

  const estWidth = 190;
  const estHeight = menuItems.reduce((h, it) => h + (it.sep ? 9 : 34), 12);
  const left = Math.min(x, window.innerWidth - estWidth - 8);
  const top = Math.min(y, window.innerHeight - estHeight - 8);
  contextMenu.style.left = `${Math.max(8, left)}px`;
  contextMenu.style.top = `${Math.max(8, top)}px`;
  contextMenu.classList.remove('hidden');
}

function showItemContextMenu(x, y, item) {
  if (selectedIds.size > 1 && selectedIds.has(item.id)) {
    showBulkContextMenu(x, y);
    return;
  }
  const menuItems = [{ label: 'Rename', action: () => openRenameModal(item) }];
  if (item.kind === 'app') {
    menuItems.push({ label: 'Edit…', action: () => openEditModal(item) });
  }
  if (item.special === 'recycleBin') {
    menuItems.push({ label: 'Empty Recycle Bin', icon: ICONS.trash, danger: true, action: () => emptyRecycleBin() });
  }
  if (item.kind === 'island') {
    menuItems.push({
      label: item.collapsed === false ? 'Minimize' : 'Expand',
      action: () => refreshFromServer(window.launcherAPI.setCollapsed(item.id, !item.collapsed)),
    });
    menuItems.push({
      label: item.showNames ? 'Hide App Names' : 'Show App Names',
      action: () => refreshFromServer(window.launcherAPI.setShowNames(item.id, !item.showNames)),
    });
  }
  menuItems.push({ label: 'Move to Island…', action: () => openMoveModal(item) });
  menuItems.push({
    label: item.pinned ? 'Unpin from Taskbar' : 'Pin to Taskbar',
    action: () => togglePin(item.id),
  });
  menuItems.push({ sep: true });
  menuItems.push({
    label: item.kind === 'island' ? 'Remove Island' : 'Remove',
    danger: true,
    action: () => removeItem(item),
  });
  openContextMenu(x, y, menuItems);
}

function showBulkContextMenu(x, y) {
  const ids = [...selectedIds];
  openContextMenu(x, y, [
    { label: `Pin ${ids.length} to Taskbar`, action: () => bulkSetPinned(ids, true) },
    { label: `Unpin ${ids.length} from Taskbar`, action: () => bulkSetPinned(ids, false) },
    { label: 'Move to Island…', action: () => openBulkMoveModal(ids) },
    { sep: true },
    { label: `Remove ${ids.length} Items`, danger: true, action: () => bulkRemove(ids) },
  ]);
}

async function bulkSetPinned(ids, pinned) {
  for (const id of ids) await window.launcherAPI.setPinned(id, pinned);
  await refreshFromServer(window.launcherAPI.listApps());
}

async function bulkRemove(ids) {
  if (!(await showConfirm(`Remove ${ids.length} items from Launchpad? (This does not delete the apps themselves.)`))) return;
  for (const id of ids) await window.launcherAPI.removeApp(id);
  clearSelection();
  await refreshFromServer(window.launcherAPI.listApps());
}

async function emptyRecycleBin() {
  if (!(await showConfirm('Permanently empty the Recycle Bin? This cannot be undone.'))) return;
  const res = await window.launcherAPI.emptyRecycleBin();
  if (!res.ok) await showAlert(`Couldn't empty the Recycle Bin:\n${res.error || 'Unknown error.'}`);
}

function showGridContextMenu(x, y) {
  openContextMenu(x, y, [
    { label: '+ New Island', action: () => openNewIslandModal() },
    { label: '+ Add App', action: () => openAddModal() },
  ]);
}

grid.addEventListener('contextmenu', (e) => {
  if (e.target === grid || e.target === canvas) {
    e.preventDefault();
    showGridContextMenu(e.clientX, e.clientY);
  }
});
emptyState.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  showGridContextMenu(e.clientX, e.clientY);
});

document.addEventListener('click', (e) => {
  if (!contextMenu.contains(e.target)) closeContextMenu();
  if (!searchResults.contains(e.target) && e.target !== taskbarSearch) {
    searchResults.classList.add('hidden');
  }
  if (!recentPopout.contains(e.target) && e.target !== recentBtn) {
    recentPopout.classList.add('hidden');
  }
  if (!volumePopout.contains(e.target) && e.target !== volumeBtn) {
    closeVolumePopout();
  }
  if (!usbPopout.contains(e.target) && e.target !== usbBtn) {
    closeUsbPopout();
  }
  if (!bluetoothPopout.contains(e.target) && e.target !== bluetoothBtn) {
    closeBluetoothPopout();
  }
  if (!networkPopout.contains(e.target) && e.target !== networkBtn) {
    closeNetworkPopout();
  }
  if (!powerPopout.contains(e.target) && e.target !== powerBtn) {
    closePowerPopout();
  }
});

// ---------- item actions ----------

async function togglePin(id) {
  await refreshFromServer(window.launcherAPI.togglePin(id));
}

async function removeItem(item) {
  const msg =
    item.kind === 'island'
      ? `Remove island "${item.name}"? Its contents will move up one level.`
      : `Remove "${item.name}" from Launchpad? (This does not delete the app itself.)`;
  if (!(await showConfirm(msg))) return;
  await refreshFromServer(window.launcherAPI.removeApp(item.id));
}

// ---------- new island / rename modal ----------

function buildColorSwatches() {
  colorSwatches.innerHTML = '';
  for (const c of ISLAND_COLORS) {
    const sw = document.createElement('button');
    sw.type = 'button';
    sw.className = 'swatch';
    sw.style.background = c;
    sw.addEventListener('click', () => setSelectedSwatch(c));
    colorSwatches.appendChild(sw);
  }
  const customSw = document.createElement('button');
  customSw.type = 'button';
  customSw.className = 'swatch custom-swatch';
  customSw.title = 'Custom color…';
  customSw.addEventListener('click', () => customIslandColorInput.click());
  colorSwatches.appendChild(customSw);
}
buildColorSwatches();

function setSelectedSwatch(color) {
  selectedIslandColor = color || ISLAND_COLORS[0];
  const isPreset = ISLAND_COLORS.includes(selectedIslandColor);
  const swatches = Array.from(colorSwatches.children);
  const customSw = swatches[swatches.length - 1];
  swatches.forEach((s, i) => {
    if (s !== customSw) s.classList.toggle('selected', ISLAND_COLORS[i] === selectedIslandColor);
  });
  customSw.classList.toggle('selected', !isPreset);
  customSw.style.background = isPreset ? '' : selectedIslandColor;
  if (!isPreset) customIslandColorInput.value = selectedIslandColor;
}

customIslandColorInput.addEventListener('input', () => {
  setSelectedSwatch(customIslandColorInput.value);
});

function openNewIslandModal() {
  nameModalMode = 'newIsland';
  nameModalTargetId = null;
  nameModalTargetKind = 'island';
  nameModalTitle.textContent = 'New Island';
  nameInput.value = '';
  colorRow.classList.remove('hidden');
  setSelectedSwatch(ISLAND_COLORS[0]);
  nameModalOverlay.classList.remove('hidden');
  nameInput.focus();
}

function openRenameModal(item) {
  nameModalMode = 'rename';
  nameModalTargetId = item.id;
  nameModalTargetKind = item.kind;
  nameModalTitle.textContent = 'Rename';
  nameInput.value = item.name;
  colorRow.classList.toggle('hidden', item.kind !== 'island');
  if (item.kind === 'island') setSelectedSwatch(item.color);
  nameModalOverlay.classList.remove('hidden');
  nameInput.focus();
  nameInput.select();
}

newFolderBtn.addEventListener('click', openNewIslandModal);
nameCancelBtn.addEventListener('click', () => nameModalOverlay.classList.add('hidden'));
nameModalOverlay.addEventListener('click', (e) => {
  if (e.target === nameModalOverlay) nameModalOverlay.classList.add('hidden');
});

nameForm.addEventListener('submit', async (e) => {
  e.preventDefault();
  const value = nameInput.value.trim();
  if (!value) return;

  nameModalOverlay.classList.add('hidden');
  if (nameModalMode === 'newIsland') {
    await refreshFromServer(window.launcherAPI.addIsland(value, currentFolderId, selectedIslandColor));
  } else {
    await refreshFromServer(window.launcherAPI.renameItem(nameModalTargetId, value));
    if (nameModalTargetKind === 'island') {
      await refreshFromServer(window.launcherAPI.setColor(nameModalTargetId, selectedIslandColor));
    }
  }
});

// ---------- move-to-island modal ----------

function openMoveModal(item) {
  moveModalItemId = item.id;
  moveModalBulkIds = null;
  moveList.innerHTML = '';

  const excluded = item.kind === 'island' ? descendantIslandIds(item.id) : new Set();
  const currentParent = item.parentId || null;

  moveList.appendChild(buildMoveRow('Top Level (Home)', null, currentParent === null));

  const islands = apps
    .filter((a) => a.kind === 'island' && a.id !== item.id && !excluded.has(a.id))
    .sort((a, b) => a.name.localeCompare(b.name));

  for (const f of islands) {
    moveList.appendChild(buildMoveRow(getPathLabel(f.id), f.id, currentParent === f.id));
  }

  moveModalOverlay.classList.remove('hidden');
}

// Same modal, but moves every id in `ids` at once - used from the bulk
// context menu on a multi-selection.
function openBulkMoveModal(ids) {
  moveModalItemId = null;
  moveModalBulkIds = ids;
  moveList.innerHTML = '';

  moveList.appendChild(buildMoveRow('Top Level (Home)', null, false));
  const islands = apps
    .filter((a) => a.kind === 'island' && !ids.includes(a.id))
    .sort((a, b) => a.name.localeCompare(b.name));
  for (const f of islands) {
    moveList.appendChild(buildMoveRow(getPathLabel(f.id), f.id, false));
  }

  moveModalOverlay.classList.remove('hidden');
}

function buildMoveRow(label, parentId, isCurrent) {
  const row = document.createElement('div');
  row.className = 'move-row' + (isCurrent ? ' disabled' : '');
  row.textContent = label + (isCurrent ? ' (current)' : '');
  row.addEventListener('click', async () => {
    if (isCurrent) return;
    moveModalOverlay.classList.add('hidden');
    if (moveModalBulkIds) {
      const ids = moveModalBulkIds;
      for (const id of ids) await window.launcherAPI.moveItem(id, parentId);
      clearSelection();
      await refreshFromServer(window.launcherAPI.listApps());
    } else {
      await refreshFromServer(window.launcherAPI.moveItem(moveModalItemId, parentId));
    }
  });
  return row;
}

moveCancelBtn.addEventListener('click', () => moveModalOverlay.classList.add('hidden'));
moveModalOverlay.addEventListener('click', (e) => {
  if (e.target === moveModalOverlay) moveModalOverlay.classList.add('hidden');
});

// ---------- settings modal ----------

function formatAccelerator(accel) {
  return accel.replace('CommandOrControl', 'Ctrl').replace('Command', 'Cmd');
}

function currentAccentColor() {
  return getComputedStyle(document.documentElement).getPropertyValue('--accent').trim();
}

settingsBtn.addEventListener('click', async () => {
  autoLaunchToggle.checked = await window.launcherAPI.getAutoLaunch();
  themeToggle.checked = document.documentElement.getAttribute('data-theme') === 'light';
  fullscreenToggle.checked = await window.launcherAPI.getStartFullscreen();
  minimizeToTrayToggle.checked = await window.launcherAPI.getMinimizeToTrayOnClose();
  showAddButtonsToggle.checked = localStorage.getItem('launchpad-hide-add-buttons') !== '1';
  accentColorInput.value = currentAccentColor();
  taskbarStyleSelect.value = localStorage.getItem('launchpad-taskbar-style') || 'solid';
  topbarStyleSelect.value = localStorage.getItem('launchpad-topbar-style') || 'solid';
  hotkeyBtn.textContent = formatAccelerator(await window.launcherAPI.getHotkey());
  settingsModalOverlay.classList.remove('hidden');
});

// Fetched once at startup (it never changes mid-session) rather than on
// every Settings open, so the label is already there with no flash.
window.launcherAPI.getVersion().then((v) => {
  versionLabel.textContent = `Launchpad v${v}`;
});
settingsCloseBtn.addEventListener('click', () => settingsModalOverlay.classList.add('hidden'));
settingsModalOverlay.addEventListener('click', (e) => {
  if (e.target === settingsModalOverlay) settingsModalOverlay.classList.add('hidden');
});

autoLaunchToggle.addEventListener('change', () => {
  window.launcherAPI.setAutoLaunch(autoLaunchToggle.checked);
});

fullscreenToggle.addEventListener('change', () => {
  window.launcherAPI.setStartFullscreen(fullscreenToggle.checked);
});

minimizeToTrayToggle.addEventListener('change', () => {
  window.launcherAPI.setMinimizeToTrayOnClose(minimizeToTrayToggle.checked);
});

showAddButtonsToggle.addEventListener('change', () => {
  const hide = !showAddButtonsToggle.checked;
  if (hide) document.documentElement.setAttribute('data-hide-add-buttons', '1');
  else document.documentElement.removeAttribute('data-hide-add-buttons');
  try {
    if (hide) localStorage.setItem('launchpad-hide-add-buttons', '1');
    else localStorage.removeItem('launchpad-hide-add-buttons');
  } catch {}
});

let capturingHotkey = false;

function acceleratorFromEvent(e) {
  const parts = [];
  if (e.ctrlKey || e.metaKey) parts.push('CommandOrControl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');

  if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return null; // pure modifier - keep waiting

  const KEY_MAP = { ' ': 'Space', ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right' };
  const mainKey = KEY_MAP[e.key] || (e.key.length === 1 ? e.key.toUpperCase() : e.key);
  parts.push(mainKey);
  return parts.join('+');
}

hotkeyBtn.addEventListener('click', () => {
  if (capturingHotkey) return;
  capturingHotkey = true;
  const previousLabel = hotkeyBtn.textContent;
  hotkeyBtn.textContent = 'Press keys… (Esc to cancel)';
  hotkeyBtn.classList.add('capturing');

  function cleanup() {
    document.removeEventListener('keydown', onKey, true);
    hotkeyBtn.classList.remove('capturing');
    capturingHotkey = false;
  }

  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      cleanup();
      hotkeyBtn.textContent = previousLabel;
      return;
    }
    const accel = acceleratorFromEvent(e);
    if (!accel) return; // still just a modifier - keep listening for the real key
    cleanup();
    window.launcherAPI.setHotkey(accel).then((res) => {
      if (res && res.ok) {
        hotkeyBtn.textContent = formatAccelerator(accel);
      } else {
        showAlert((res && res.error) || "Couldn't set that hotkey.");
        hotkeyBtn.textContent = previousLabel;
      }
    });
  }

  // Capture phase so this intercepts the keystroke before the global
  // type-to-search / arrow-nav listeners can react to it too.
  document.addEventListener('keydown', onKey, true);
});

themeToggle.addEventListener('change', () => {
  const light = themeToggle.checked;
  document.documentElement.setAttribute('data-theme', light ? 'light' : 'dark');
  try {
    localStorage.setItem('launchpad-theme', light ? 'light' : 'dark');
  } catch {}
});

accentColorInput.addEventListener('input', () => {
  document.documentElement.style.setProperty('--accent', accentColorInput.value);
  try {
    localStorage.setItem('launchpad-accent', accentColorInput.value);
  } catch {}
});

accentResetBtn.addEventListener('click', () => {
  document.documentElement.style.removeProperty('--accent');
  try {
    localStorage.removeItem('launchpad-accent');
  } catch {}
  accentColorInput.value = currentAccentColor();
});

function applyBarStyle(el, storageKey, style) {
  if (style === 'solid') el.removeAttribute('data-style');
  else el.setAttribute('data-style', style);
  try {
    if (style === 'solid') localStorage.removeItem(storageKey);
    else localStorage.setItem(storageKey, style);
  } catch {}
}

taskbarStyleSelect.addEventListener('change', () => {
  applyBarStyle(taskbarEl, 'launchpad-taskbar-style', taskbarStyleSelect.value);
});

topbarStyleSelect.addEventListener('change', () => {
  applyBarStyle(toolbarEl, 'launchpad-topbar-style', topbarStyleSelect.value);
});

wallpaperPickBtn.addEventListener('click', () => wallpaperFileInput.click());

wallpaperFileInput.addEventListener('change', () => {
  const file = wallpaperFileInput.files && wallpaperFileInput.files[0];
  wallpaperFileInput.value = '';
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => {
    const dataUrl = reader.result;
    document.documentElement.style.setProperty('--wallpaper', `url("${dataUrl}")`);
    try {
      localStorage.setItem('launchpad-wallpaper', dataUrl);
    } catch {
      showAlert("That image is too large to save as a wallpaper - try a smaller file.");
      document.documentElement.style.removeProperty('--wallpaper');
    }
  };
  reader.readAsDataURL(file);
});

wallpaperResetBtn.addEventListener('click', () => {
  document.documentElement.style.removeProperty('--wallpaper');
  try {
    localStorage.removeItem('launchpad-wallpaper');
  } catch {}
});

detectIconsBtn.addEventListener('click', async () => {
  detectIconsBtn.disabled = true;
  const originalLabel = detectIconsBtn.textContent;
  detectIconsBtn.textContent = 'Scanning…';
  try {
    const res = await window.launcherAPI.detectMissingIcons();
    apps = res.apps;
    render();
    renderPinned();
    await showAlert(res.fixed > 0 ? `Fixed ${res.fixed} icon${res.fixed === 1 ? '' : 's'}.` : 'No missing icons found.');
  } finally {
    detectIconsBtn.disabled = false;
    detectIconsBtn.textContent = originalLabel;
  }
});

const restoreDefaultsBtn = document.getElementById('restoreDefaultsBtn');
restoreDefaultsBtn.addEventListener('click', async () => {
  restoreDefaultsBtn.disabled = true;
  const originalLabel = restoreDefaultsBtn.textContent;
  restoreDefaultsBtn.textContent = 'Adding…';
  try {
    const res = await window.launcherAPI.restoreDefaults();
    apps = res.apps;
    render();
    renderPinned();
    await showAlert(res.added > 0 ? `Added ${res.added} default app${res.added === 1 ? '' : 's'}.` : 'Already on your list.');
  } finally {
    restoreDefaultsBtn.disabled = false;
    restoreDefaultsBtn.textContent = originalLabel;
  }
});

exportBtn.addEventListener('click', async () => {
  const res = await window.launcherAPI.exportBackup();
  if (res && res.ok) await showAlert(`Exported to:\n${res.path}`);
});

importBtn.addEventListener('click', async () => {
  if (!(await showConfirm('Importing will replace everything currently in Launchpad with this backup. Continue?'))) return;
  const res = await window.launcherAPI.importBackup();
  if (res && res.ok) {
    apps = res.apps;
    currentFolderId = null;
    render();
    renderPinned();
    settingsModalOverlay.classList.add('hidden');
  }
});

// ---------- add / edit app modal ----------

function setTypeVisibility(type) {
  targetRow.classList.toggle('hidden', type === 'url' || type === 'command');
  urlRow.classList.toggle('hidden', type !== 'url');
  scriptRow.classList.toggle('hidden', type !== 'script');
  commandRow.classList.toggle('hidden', type !== 'command');
  targetLabel.firstChild.textContent = type === 'script' ? 'Script File' : 'Target';
  browseFolderBtn.classList.toggle('hidden', type !== 'path');
}

fType.addEventListener('change', () => setTypeVisibility(fType.value));

function resetForm() {
  appForm.reset();
  editingId = null;
  pendingCustomIconPath = null;
  pendingAutoIcon = false;
  iconPreview.src = PLACEHOLDER_ICON;
  fType.value = 'path';
  setTypeVisibility('path');
  deleteBtn.classList.add('hidden');
}

function openAddModal() {
  resetForm();
  modalTitle.textContent = 'Add App';
  modalOverlay.classList.remove('hidden');
  fName.focus();
}

function openEditModal(app) {
  resetForm();
  editingId = app.id;
  modalTitle.textContent = 'Edit App';
  deleteBtn.classList.remove('hidden');

  fName.value = app.name || '';
  fType.value = app.type || 'path';
  setTypeVisibility(fType.value);
  fTarget.value = app.target || '';
  fUrl.value = app.type === 'url' ? app.target || '' : '';
  fInterpreter.value = app.interpreter || '';
  fArgs.value = app.args || '';
  fCwdScript.value = app.cwd || '';
  fCommand.value = app.command || '';
  fCwdCommand.value = app.cwd || '';
  iconPreview.src = app.iconDataUrl || PLACEHOLDER_ICON;

  modalOverlay.classList.remove('hidden');
}

function closeModal() {
  modalOverlay.classList.add('hidden');
}

addBtn.addEventListener('click', openAddModal);
cancelBtn.addEventListener('click', closeModal);
modalOverlay.addEventListener('click', (e) => {
  if (e.target === modalOverlay) closeModal();
});

async function pickAndFillTarget(kind) {
  const result = await window.launcherAPI.pickTarget(kind);
  if (!result) return;
  fTarget.value = result.target;
  fType.value = result.type;
  setTypeVisibility(result.type);
  if (result.interpreter) fInterpreter.value = result.interpreter;
  if (!fName.value) fName.value = result.name;
  if (result.iconDataUrl && !pendingCustomIconPath) {
    // A new target was picked, so its icon should win on save even if this
    // entry previously had a deliberately-chosen custom icon.
    iconPreview.src = result.iconDataUrl;
    pendingAutoIcon = true;
  }
}

browseBtn.addEventListener('click', () => pickAndFillTarget('file'));
browseFolderBtn.addEventListener('click', () => pickAndFillTarget('folder'));

autoIconBtn.addEventListener('click', async () => {
  let dataUrl = null;

  if (fType.value === 'url') {
    const url = fUrl.value.trim();
    if (!url) {
      await showAlert('Enter a URL first, then auto-detect its icon.');
      return;
    }
    dataUrl = await window.launcherAPI.fetchFaviconPreview(url);
    if (!dataUrl) await showAlert("Couldn't find an icon for that website.");
  } else {
    const target = fTarget.value.trim();
    if (!target) {
      await showAlert('Set a target file first, then auto-detect its icon.');
      return;
    }
    dataUrl = await window.launcherAPI.extractIconPreview(target);
  }

  if (!dataUrl) return; // detection failed - leave whatever icon is already set alone
  pendingCustomIconPath = null;
  pendingAutoIcon = true;
  iconPreview.src = dataUrl;
});

chooseIconBtn.addEventListener('click', async () => {
  const result = await window.launcherAPI.pickIcon();
  if (!result) return;
  pendingCustomIconPath = result.path;
  pendingAutoIcon = false;
  if (result.iconDataUrl) iconPreview.src = result.iconDataUrl;
});

deleteBtn.addEventListener('click', async () => {
  if (!editingId) return;
  if (!(await showConfirm('Remove this app from the launcher? (This does not delete the app itself.)'))) return;
  closeModal();
  await refreshFromServer(window.launcherAPI.removeApp(editingId));
});

appForm.addEventListener('submit', async (e) => {
  e.preventDefault();
  const type = fType.value;

  const formData = {
    name: fName.value.trim(),
    type,
    target: type === 'url' ? fUrl.value.trim() : fTarget.value.trim(),
    args: type === 'script' ? fArgs.value.trim() : '',
    cwd: type === 'script' ? fCwdScript.value.trim() : type === 'command' ? fCwdCommand.value.trim() : '',
    interpreter: type === 'script' ? fInterpreter.value.trim() || 'python' : '',
    command: type === 'command' ? fCommand.value.trim() : '',
    customIconPath: pendingCustomIconPath || undefined,
    forceAutoIcon: pendingAutoIcon,
    parentId: currentFolderId,
  };

  if (!formData.name) {
    await showAlert('Give it a name.');
    return;
  }
  if (type !== 'command' && !formData.target) {
    await showAlert('Set a target.');
    return;
  }
  if (type === 'command' && !formData.command) {
    await showAlert('Enter a command.');
    return;
  }

  closeModal();
  if (editingId) {
    await refreshFromServer(window.launcherAPI.updateApp(editingId, formData));
  } else {
    await refreshFromServer(window.launcherAPI.addApp(formData));
  }
});

// ---------- Escape handling ----------

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  if (!dialogOverlay.classList.contains('hidden')) closeDialog(false);
  if (!modalOverlay.classList.contains('hidden')) closeModal();
  if (!nameModalOverlay.classList.contains('hidden')) nameModalOverlay.classList.add('hidden');
  if (!moveModalOverlay.classList.contains('hidden')) moveModalOverlay.classList.add('hidden');
  if (!settingsModalOverlay.classList.contains('hidden')) settingsModalOverlay.classList.add('hidden');
  closeContextMenu();
  searchResults.classList.add('hidden');
  recentPopout.classList.add('hidden');
  closeVolumePopout();
  closeUsbPopout();
  closeBluetoothPopout();
  closeNetworkPopout();
  closePowerPopout();
  clearSelection();
});

refreshFromServer(window.launcherAPI.listApps());
