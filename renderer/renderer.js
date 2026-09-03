const grid = document.getElementById('grid');
const emptyState = document.getElementById('empty');
const emptyTitle = document.getElementById('emptyTitle');
const emptySubtitle = document.getElementById('emptySubtitle');
const breadcrumbEl = document.getElementById('breadcrumb');
const addBtn = document.getElementById('addBtn');
const newFolderBtn = document.getElementById('newFolderBtn');

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

const moveModalOverlay = document.getElementById('moveModalOverlay');
const moveList = document.getElementById('moveList');
const moveCancelBtn = document.getElementById('moveCancelBtn');

const taskbar = document.getElementById('taskbar');
const pinnedRow = document.getElementById('pinnedRow');
const pinnedEmpty = document.getElementById('pinnedEmpty');
const taskbarSearch = document.getElementById('taskbarSearch');
const searchResults = document.getElementById('searchResults');

const PLACEHOLDER_ICON =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect width="48" height="48" rx="10" fill="#2b2f38"/><text x="24" y="30" font-size="20" text-anchor="middle" fill="#9aa0ac" font-family="sans-serif">?</text></svg>'
  );

let apps = [];
let currentFolderId = null;
let editingId = null;
let pendingCustomIconPath = null;
let nameModalMode = null; // 'newFolder' | 'rename'
let nameModalTargetId = null;
let moveModalItemId = null;

// ---------- helpers ----------

function iconNode(item, extraClass) {
  let el;
  if (item.kind === 'folder') {
    el = document.createElement('div');
    el.className = 'folder-icon';
    el.textContent = '📁';
  } else {
    el = document.createElement('img');
    el.alt = '';
    el.src = item.iconDataUrl || PLACEHOLDER_ICON;
  }
  if (extraClass) el.classList.add(extraClass);
  return el;
}

function getAncestors(folderId) {
  const chain = [];
  let cur = folderId;
  while (cur) {
    const f = apps.find((a) => a.id === cur && a.kind === 'folder');
    if (!f) break;
    chain.unshift(f);
    cur = f.parentId || null;
  }
  return chain;
}

function getPathLabel(folderId) {
  const chain = getAncestors(folderId);
  return chain.length ? chain.map((f) => f.name).join(' / ') : 'Home';
}

function descendantFolderIds(folderId) {
  const ids = new Set([folderId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const a of apps) {
      if (a.kind === 'folder' && ids.has(a.parentId) && !ids.has(a.id)) {
        ids.add(a.id);
        grew = true;
      }
    }
  }
  return ids;
}

async function refreshFromServer(promise) {
  apps = await promise;
  // If the folder we're standing in just got removed, bump back to Home
  // instead of showing a dead end.
  if (currentFolderId && !apps.some((a) => a.id === currentFolderId && a.kind === 'folder')) {
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
    breadcrumbEl.appendChild(btn);
  });
}

function buildTile(item) {
  const tile = document.createElement('div');
  tile.className = 'tile';
  tile.title = item.name;

  tile.appendChild(iconNode(item, 'tile-icon'));

  const label = document.createElement('div');
  label.className = 'tile-name';
  label.textContent = item.name;
  tile.appendChild(label);

  if (item.pinned) {
    const badge = document.createElement('div');
    badge.className = 'pin-badge';
    badge.textContent = '📌';
    tile.appendChild(badge);
  }

  const editBtn = document.createElement('button');
  editBtn.className = 'edit-btn';
  editBtn.type = 'button';
  editBtn.textContent = '✎';
  editBtn.title = 'Edit';
  editBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    if (item.kind === 'folder') openRenameModal(item);
    else openEditModal(item);
  });
  tile.appendChild(editBtn);

  tile.addEventListener('click', () => {
    if (item.kind === 'folder') {
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

  return tile;
}

function render() {
  renderBreadcrumb();

  const items = apps.filter((a) => (a.parentId || null) === currentFolderId);
  items.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'folder' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  grid.innerHTML = '';
  const isEmpty = items.length === 0;
  emptyState.classList.toggle('hidden', !isEmpty);
  grid.classList.toggle('hidden', isEmpty);

  if (isEmpty) {
    emptyTitle.textContent = currentFolderId ? 'This folder is empty.' : 'No apps yet.';
    emptySubtitle.innerHTML = currentFolderId
      ? 'Right-click to add something, or click <strong>+ Add App</strong>.'
      : 'Click <strong>+ Add App</strong> to add your first one, or right-click for a folder.';
  }

  for (const item of items) {
    grid.appendChild(buildTile(item));
  }
}

async function launchApp(item) {
  try {
    await window.launcherAPI.launchApp(item.id);
  } catch (err) {
    alert(`Couldn't launch "${item.name}":\n${err.message || err}`);
  }
}

// ---------- taskbar: pinned row ----------

function renderPinned() {
  const pinned = apps.filter((a) => a.pinned);
  pinnedRow.innerHTML = '';
  pinnedEmpty.classList.toggle('hidden', pinned.length > 0);

  for (const item of pinned) {
    const el = document.createElement('div');
    el.className = 'pin-tile';
    el.title = item.name;
    el.appendChild(iconNode(item));
    el.addEventListener('click', () => {
      if (item.kind === 'folder') {
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
    pinnedRow.appendChild(el);
  }
}

// ---------- taskbar: search ----------

function renderSearchResults(matches) {
  searchResults.innerHTML = '';
  if (matches.length === 0) {
    const div = document.createElement('div');
    div.className = 'search-empty';
    div.textContent = 'No matches.';
    searchResults.appendChild(div);
  } else {
    for (const item of matches) {
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

      row.addEventListener('click', () => selectSearchResult(item));
      searchResults.appendChild(row);
    }
  }
  searchResults.classList.remove('hidden');
}

async function selectSearchResult(item) {
  searchResults.classList.add('hidden');
  taskbarSearch.value = '';
  taskbarSearch.blur();
  if (item.kind === 'folder') {
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
  const matches = apps.filter((a) => a.name.toLowerCase().includes(q)).slice(0, 20);
  renderSearchResults(matches);
});

taskbarSearch.addEventListener('focus', () => {
  if (taskbarSearch.value.trim()) searchResults.classList.remove('hidden');
});

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
    btn.textContent = it.label;
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
  const menuItems = [{ label: 'Rename', action: () => openRenameModal(item) }];
  if (item.kind === 'app') {
    menuItems.push({ label: 'Edit…', action: () => openEditModal(item) });
  }
  menuItems.push({ label: 'Move to Folder…', action: () => openMoveModal(item) });
  menuItems.push({
    label: item.pinned ? 'Unpin from Taskbar' : 'Pin to Taskbar',
    action: () => togglePin(item.id),
  });
  menuItems.push({ sep: true });
  menuItems.push({
    label: item.kind === 'folder' ? 'Remove Folder' : 'Remove',
    danger: true,
    action: () => removeItem(item),
  });
  openContextMenu(x, y, menuItems);
}

function showGridContextMenu(x, y) {
  openContextMenu(x, y, [
    { label: '+ New Folder', action: () => openNewFolderModal() },
    { label: '+ Add App', action: () => openAddModal() },
  ]);
}

grid.addEventListener('contextmenu', (e) => {
  if (e.target === grid) {
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
});

// ---------- item actions ----------

async function togglePin(id) {
  await refreshFromServer(window.launcherAPI.togglePin(id));
}

async function removeItem(item) {
  const msg =
    item.kind === 'folder'
      ? `Remove folder "${item.name}"? Its contents will move up one level.`
      : `Remove "${item.name}" from Launchpad? (This does not delete the app itself.)`;
  if (!confirm(msg)) return;
  await refreshFromServer(window.launcherAPI.removeApp(item.id));
}

// ---------- new folder / rename modal ----------

function openNewFolderModal() {
  nameModalMode = 'newFolder';
  nameModalTargetId = null;
  nameModalTitle.textContent = 'New Folder';
  nameInput.value = '';
  nameModalOverlay.classList.remove('hidden');
  nameInput.focus();
}

function openRenameModal(item) {
  nameModalMode = 'rename';
  nameModalTargetId = item.id;
  nameModalTitle.textContent = 'Rename';
  nameInput.value = item.name;
  nameModalOverlay.classList.remove('hidden');
  nameInput.focus();
  nameInput.select();
}

newFolderBtn.addEventListener('click', openNewFolderModal);
nameCancelBtn.addEventListener('click', () => nameModalOverlay.classList.add('hidden'));
nameModalOverlay.addEventListener('click', (e) => {
  if (e.target === nameModalOverlay) nameModalOverlay.classList.add('hidden');
});

nameForm.addEventListener('submit', async (e) => {
  e.preventDefault();
  const value = nameInput.value.trim();
  if (!value) return;

  nameModalOverlay.classList.add('hidden');
  if (nameModalMode === 'newFolder') {
    await refreshFromServer(window.launcherAPI.addFolder(value, currentFolderId));
  } else {
    await refreshFromServer(window.launcherAPI.renameItem(nameModalTargetId, value));
  }
});

// ---------- move-to-folder modal ----------

function openMoveModal(item) {
  moveModalItemId = item.id;
  moveList.innerHTML = '';

  const excluded = item.kind === 'folder' ? descendantFolderIds(item.id) : new Set();
  const currentParent = item.parentId || null;

  moveList.appendChild(buildMoveRow('Top Level (Home)', null, currentParent === null));

  const folders = apps
    .filter((a) => a.kind === 'folder' && a.id !== item.id && !excluded.has(a.id))
    .sort((a, b) => a.name.localeCompare(b.name));

  for (const f of folders) {
    moveList.appendChild(buildMoveRow(getPathLabel(f.id), f.id, currentParent === f.id));
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
    await refreshFromServer(window.launcherAPI.moveItem(moveModalItemId, parentId));
  });
  return row;
}

moveCancelBtn.addEventListener('click', () => moveModalOverlay.classList.add('hidden'));
moveModalOverlay.addEventListener('click', (e) => {
  if (e.target === moveModalOverlay) moveModalOverlay.classList.add('hidden');
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
  if (result.iconDataUrl && !pendingCustomIconPath) iconPreview.src = result.iconDataUrl;
}

browseBtn.addEventListener('click', () => pickAndFillTarget('file'));
browseFolderBtn.addEventListener('click', () => pickAndFillTarget('folder'));

autoIconBtn.addEventListener('click', async () => {
  const target = fType.value === 'url' ? '' : fTarget.value;
  if (!target) {
    alert('Set a target file first, then auto-detect its icon.');
    return;
  }
  const dataUrl = await window.launcherAPI.extractIconPreview(target);
  pendingCustomIconPath = null;
  iconPreview.src = dataUrl || PLACEHOLDER_ICON;
});

chooseIconBtn.addEventListener('click', async () => {
  const result = await window.launcherAPI.pickIcon();
  if (!result) return;
  pendingCustomIconPath = result.path;
  if (result.iconDataUrl) iconPreview.src = result.iconDataUrl;
});

deleteBtn.addEventListener('click', async () => {
  if (!editingId) return;
  if (!confirm('Remove this app from the launcher? (This does not delete the app itself.)')) return;
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
    refreshIcon: !pendingCustomIconPath,
    parentId: currentFolderId,
  };

  if (!formData.name) {
    alert('Give it a name.');
    return;
  }
  if (type !== 'command' && !formData.target) {
    alert('Set a target.');
    return;
  }
  if (type === 'command' && !formData.command) {
    alert('Enter a command.');
    return;
  }

  closeModal();
  if (editingId) {
    await refreshFromServer(window.launcherAPI.updateApp(editingId, formData));
  } else {
    await refreshFromServer(window.launcherAPI.addApp(formData));
  }
});

// ---------- global keys ----------

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  if (!modalOverlay.classList.contains('hidden')) closeModal();
  if (!nameModalOverlay.classList.contains('hidden')) nameModalOverlay.classList.add('hidden');
  if (!moveModalOverlay.classList.contains('hidden')) moveModalOverlay.classList.add('hidden');
  closeContextMenu();
  searchResults.classList.add('hidden');
});

refreshFromServer(window.launcherAPI.listApps());
