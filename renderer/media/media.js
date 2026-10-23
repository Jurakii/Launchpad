// Media's screens: the library (Home), Add library folder, Players and
// emulators, and the screen shown while something runs. Everything goes
// through window.mediaAPI (media-preload.js); focus moves with
// window.MediaFocus (focus.js).

// Same look as the main window: its theme and accent live in localStorage,
// which this page shares (same file:// origin).
try {
  if (localStorage.getItem('launchpad-theme') === 'light') document.documentElement.setAttribute('data-theme', 'light');
  const accent = localStorage.getItem('launchpad-accent');
  if (accent) document.documentElement.style.setProperty('--accent', accent);
} catch {}

const api = window.mediaAPI;
const $ = (id) => document.getElementById(id);

const state = {
  tab: 'games',
  entries: [],
  platforms: new Map(), // id -> name
  profiles: { profiles: [], defaults: {}, presets: [], detected: [] },
  screen: 'home',
  input: 'keys', // 'keys' | 'pad': which glyphs the prompt bar shows
  homeFocus: null, // the tile to return focus to
  running: null, // { id, name, startedAt }
  add: { root: null, library: 'games', recursive: true, platforms: {} },
  editing: null, // profile being edited, or null
  customizing: null, // id of the entry on the Customize screen
  controller: { deadZone: 0.35, pads: {} }, // settings.json `controller`
  capturing: null, // action waiting for a button on the Controller screen
};

// "Error invoking remote method 'media:launch': Error: Can't find…" -> the message.
const message = (err) => String((err && err.message) || err).replace(/^Error invoking remote method '[^']+': (Error: )?/, '');

let toastTimer = null;
function toast(text) {
  const el = $('toast');
  el.textContent = text;
  el.classList.remove('hidden');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add('hidden'), 4000);
}

// ---------- Screens ----------

const SCREENS = {
  home: 'homeScreen',
  add: 'addScreen',
  profiles: 'profilesScreen',
  customize: 'customizeScreen',
  controller: 'controllerScreen',
  running: 'runningScreen',
};

function showScreen(name) {
  if (state.screen === 'home' && document.activeElement && document.activeElement.classList.contains('tile')) {
    state.homeFocus = document.activeElement;
  }
  state.screen = name;
  for (const [key, id] of Object.entries(SCREENS)) $(id).classList.toggle('hidden', key !== name);
  document.body.dataset.screen = name;
  renderPrompts();
  MediaFocus.enter($(SCREENS[name]), name === 'home' ? state.homeFocus : null);
}

function renderPrompts() {
  MediaPrompts.render($('prompts'), state.screen, state.input, padGlyphs());
}

// The prompts follow whatever was used last: keys and mouse, or a pad.
function setInput(input) {
  if (state.input === input) return;
  state.input = input;
  document.body.dataset.input = input;
  renderPrompts();
}
document.addEventListener('keydown', () => setInput('keys'), true);
document.addEventListener('pointerdown', () => setInput('keys'), true);

function back() {
  if (state.capturing) return cancelCapture();
  if (state.screen === 'home') api.close();
  else showScreen('home');
}

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  e.preventDefault();
  back();
});

// A file dropped anywhere but an art slot would otherwise make the window
// navigate to it, leaving the library behind.
document.addEventListener('dragover', (e) => e.preventDefault());
document.addEventListener('drop', (e) => e.preventDefault());

for (const btn of document.querySelectorAll('.back-btn')) btn.addEventListener('click', () => showScreen('home'));
$('exitBtn').addEventListener('click', () => api.close());
$('addFolderBtn').addEventListener('click', openAdd);
$('emptyAddBtn').addEventListener('click', openAdd);
$('profilesBtn').addEventListener('click', openProfiles);
$('controllerBtn').addEventListener('click', openController);

function tickClock() {
  $('clock').textContent = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}
tickClock();
setInterval(tickClock, 30_000);

// ---------- Library data ----------

async function loadLibrary() {
  const { entries, loadIssue } = await api.library();
  state.entries = entries;
  if (loadIssue && loadIssue.state === 'readOnly') {
    toast("library.json couldn't be read, so changes won't be saved until it is fixed.");
  }
}

async function loadProfiles() {
  state.profiles = await api.profiles();
}

const platformName = (id) => (id ? state.platforms.get(id) || id : 'Not detected');

// The profile an entry would launch with, mirroring players.profileFor().
function profileFor(entry) {
  const { profiles, defaults } = state.profiles;
  const id = entry.profile || defaults[entry.library === 'videos' ? 'videos' : entry.platform];
  return profiles.find((p) => p.id === id) || null;
}

function defaultProfileName(key) {
  const id = state.profiles.defaults[key];
  const p = state.profiles.profiles.find((x) => x.id === id);
  return p ? p.name : null;
}

// ---------- Home ----------

for (const tab of document.querySelectorAll('.tab')) {
  tab.addEventListener('click', () => {
    state.tab = tab.dataset.tab;
    state.homeFocus = null;
    renderHome();
    MediaFocus.enter($('homeScreen'));
  });
}

const byName = (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true });

function homeRows() {
  const shown = state.entries.filter((e) => state.tab === 'all' || e.library === state.tab);
  const rows = [];
  const played = shown
    .filter((e) => e.launchCount > 0)
    .sort((a, b) => (b.lastLaunched || 0) - (a.lastLaunched || 0))
    .slice(0, 12);
  if (played.length) rows.push({ title: 'Continue', items: played });

  const games = shown.filter((e) => e.library === 'games');
  const groups = new Map();
  for (const g of games) {
    const key = g.platform || '';
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(g);
  }
  const keys = [...groups.keys()].sort((a, b) => (!a ? 1 : !b ? -1 : platformName(a).localeCompare(platformName(b))));
  for (const key of keys) rows.push({ title: platformName(key || null), items: groups.get(key).sort(byName) });

  const videos = shown.filter((e) => e.library === 'videos');
  if (videos.length) {
    const recent = [...videos].sort((a, b) => (b.createdAt || 0) - (a.createdAt || 0)).slice(0, 24);
    rows.push({ title: 'Recently added', items: recent, wide: true });
    rows.push({ title: 'All videos', items: [...videos].sort(byName), wide: true });
  }
  return rows;
}

// lpmedia: URL for one slot's art, or null. `v` changes with the art.
function artUrl(entry, slot) {
  const v = entry.art && entry.art[slot];
  return v === undefined || v === null ? null : `lpmedia://art/${entry.id}/${slot}?v=${v}`;
}

// The picture a tile shows: covers for games, banners (or a screenshot) for videos.
function tileArt(entry, wide) {
  const order = wide ? ['banner', 'screenshot', 'cover'] : ['cover', 'banner', 'screenshot'];
  for (const slot of order) {
    const url = artUrl(entry, slot);
    if (url) return url;
  }
  return null;
}

function tileFor(entry, wide) {
  const isWide = wide || entry.library === 'videos';
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = `tile${isWide ? ' wide' : ''}${entry.available ? '' : ' unavailable'}`;
  btn.dataset.nav = '';
  btn.dataset.id = entry.id;
  const url = tileArt(entry, isWide);
  if (url) {
    const img = document.createElement('img');
    img.className = 'tile-img';
    img.alt = '';
    img.loading = 'lazy';
    img.src = url;
    btn.appendChild(img);
    btn.classList.add('has-art');
  }
  const name = document.createElement('span');
  name.className = 'tile-name';
  name.textContent = entry.name;
  btn.appendChild(name);
  if (!entry.available) {
    const flag = document.createElement('span');
    flag.className = 'tile-flag';
    flag.textContent = 'Not found';
    btn.appendChild(flag);
  }
  btn.setAttribute('aria-label', entry.available ? entry.name : `${entry.name}, file not found`);
  btn.addEventListener('focus', () => showHero(entry));
  btn.addEventListener('click', () => launch(entry));
  btn.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    openCustomize(entry.id);
  });
  btn.addEventListener('keydown', (e) => {
    if (e.key === 'c' || e.key === 'C') {
      e.preventDefault();
      openCustomize(entry.id);
    }
  });
  return btn;
}

function renderHome() {
  for (const tab of document.querySelectorAll('.tab')) {
    tab.classList.toggle('active', tab.dataset.tab === state.tab);
    tab.setAttribute('aria-pressed', String(tab.dataset.tab === state.tab));
  }
  const rows = homeRows();
  const container = $('rows');
  container.replaceChildren();
  for (const row of rows) {
    const section = document.createElement('section');
    section.className = 'row';
    const h = document.createElement('h2');
    h.textContent = row.title;
    const strip = document.createElement('div');
    strip.className = 'strip';
    for (const entry of row.items) strip.appendChild(tileFor(entry, row.wide));
    section.append(h, strip);
    container.appendChild(section);
  }
  const empty = rows.length === 0;
  $('emptyState').classList.toggle('hidden', !empty);
  $('hero').classList.toggle('hidden', empty);
  $('emptyTitle').textContent =
    state.tab === 'videos' ? 'No videos yet' : state.tab === 'games' ? 'No games yet' : 'Nothing here yet';
  if (!empty) showHero(rows[0].items[0]);
  // A re-render replaces the tiles: keep focus on the same entry.
  if (state.homeFocus && !state.homeFocus.isConnected) {
    state.homeFocus = container.querySelector(`[data-id="${state.homeFocus.dataset.id}"]`);
  }
}

function showHero(entry) {
  $('heroName').textContent = entry.name;
  const info = [];
  info.push(entry.library === 'videos' ? 'Video' : platformName(entry.platform));
  if (entry.meta && entry.meta.year) info.push(String(entry.meta.year));
  if (entry.launchCount) info.push(entry.launchCount === 1 ? 'Played once' : `Played ${entry.launchCount} times`);
  if (entry.playSeconds >= 60) info.push(`${formatDuration(entry.playSeconds)} in total`);
  if (!entry.available) info.push('File not found: rescan the folder or check the drive');
  const profile = profileFor(entry);
  if (entry.available && entry.type === 'profile') info.push(profile ? `Opens in ${profile.name}` : 'No player or emulator set up');
  $('heroInfo').textContent = info.join(' · ');
  const wide = entry.library === 'videos';
  const picture = wide ? artUrl(entry, 'banner') || artUrl(entry, 'screenshot') : artUrl(entry, 'cover');
  const heroArt = $('heroArt');
  heroArt.classList.toggle('wide', wide);
  heroArt.replaceChildren();
  if (picture) {
    const img = document.createElement('img');
    img.alt = '';
    img.src = picture;
    heroArt.appendChild(img);
  } else {
    heroArt.textContent = entry.name;
  }
  // A logo stands in for the name, which stays for screen readers.
  const logo = artUrl(entry, 'logo');
  $('heroLogo').classList.toggle('hidden', !logo);
  $('heroName').classList.toggle('visually-hidden', !!logo);
  if (logo) $('heroLogo').src = logo;
  // The banner fills the screen behind the library, faintly.
  const backdrop = artUrl(entry, 'banner');
  document.body.style.setProperty('--backdrop', backdrop ? `url("${backdrop}")` : 'none');
}

function formatDuration(seconds) {
  if (seconds < 60) return 'under a minute';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h ? `${h}h ${String(m).padStart(2, '0')}m` : `${m}m`;
}

// ---------- Launching ----------

let runningTimer = null;

async function launch(entry) {
  try {
    const result = await api.launch(entry.id);
    if (result.tracked || result.alreadyRunning) {
      const profile = profileFor(entry);
      if (!result.alreadyRunning || !state.running || state.running.id !== entry.id) {
        state.running = { id: entry.id, name: entry.name, with: profile ? profile.name : '', startedAt: Date.now() };
      }
      showRunning();
    } else {
      toast(`Started ${entry.name}`);
    }
    await refreshAfterLaunch();
  } catch (err) {
    toast(message(err));
  }
}

function showRunning() {
  const r = state.running;
  $('runningName').textContent = r.name;
  $('runningArt').textContent = r.name;
  $('runningWith').textContent = r.with ? `Running in ${r.with}` : 'Running';
  const tick = () => {
    $('runningTime').textContent = `Running for ${formatDuration(Math.round((Date.now() - r.startedAt) / 1000))}`;
  };
  tick();
  clearInterval(runningTimer);
  runningTimer = setInterval(tick, 15_000);
  showScreen('running');
}

$('runningStopBtn').addEventListener('click', () => {
  if (state.running) api.stop(state.running.id);
});

api.onExited(async (info) => {
  const wasRunning = state.running && state.running.id === info.id;
  const name = wasRunning ? state.running.name : 'It';
  if (wasRunning) {
    state.running = null;
    clearInterval(runningTimer);
  }
  // A hand-off passed the game to another program, which keeps it running.
  if (info.handoff) toast(`${name} was handed to another program`);
  await refreshAfterLaunch();
  if (wasRunning && state.screen === 'running') showScreen('home');
});

async function refreshAfterLaunch() {
  await loadLibrary();
  renderHome();
}

// ---------- Add library folder ----------

function openAdd() {
  state.add = { root: null, library: 'games', recursive: true, platforms: {} };
  $('addRoot').value = '';
  $('addType').value = 'games';
  $('addRecursive').checked = true;
  $('addStatus').textContent = 'Choose a folder to see what is in it.';
  $('addTable').classList.add('hidden');
  $('addConfirmBtn').disabled = true;
  showScreen('add');
}

$('addBrowseBtn').addEventListener('click', async () => {
  const root = await api.pickFolder();
  if (!root) return;
  state.add.root = root;
  state.add.platforms = {};
  $('addRoot').value = root;
  preview();
});
$('addType').addEventListener('change', () => {
  state.add.library = $('addType').value;
  state.add.platforms = {};
  preview();
});
$('addRecursive').addEventListener('change', () => {
  state.add.recursive = $('addRecursive').checked;
  preview();
});

let previewRun = 0;
async function preview() {
  if (!state.add.root) return;
  const run = ++previewRun;
  $('addStatus').textContent = 'Looking through the folder…';
  $('addConfirmBtn').disabled = true;
  try {
    const result = await api.scanPreview(state.add);
    if (run !== previewRun) return; // a newer preview started
    renderPreview(result);
  } catch (err) {
    if (run !== previewRun) return;
    $('addStatus').textContent = message(err);
    $('addTable').classList.add('hidden');
  }
}

function renderPreview({ count, groups }) {
  const videos = state.add.library === 'videos';
  const noun = videos ? (count === 1 ? 'video' : 'videos') : count === 1 ? 'game' : 'games';
  $('addStatus').textContent = count ? `Found ${count} ${noun}.` : `No ${noun} found in this folder.`;
  $('addConfirmBtn').disabled = count === 0;
  $('addTable').classList.toggle('hidden', count === 0);

  const tbody = $('addRows');
  tbody.replaceChildren();
  for (const g of groups) {
    const tr = document.createElement('tr');
    const folder = document.createElement('td');
    folder.className = 'mono';
    folder.textContent = g.folder ? `${g.folder}\\` : '(top folder)';

    const platform = document.createElement('td');
    if (videos) {
      platform.textContent = 'Video';
    } else {
      const select = document.createElement('select');
      select.dataset.nav = '';
      select.setAttribute('aria-label', `Platform for ${folder.textContent}`);
      select.add(new Option('Not detected', ''));
      for (const [id, name] of state.platforms) select.add(new Option(name, id));
      select.value = g.platform || '';
      if (!g.platform) select.classList.add('warn');
      select.addEventListener('change', () => {
        if (select.value) state.add.platforms[g.folder] = select.value;
        else delete state.add.platforms[g.folder];
        preview();
      });
      platform.appendChild(select);
    }

    const files = document.createElement('td');
    files.textContent = String(g.count);

    const launchWith = document.createElement('td');
    const key = videos ? 'videos' : g.platform;
    const name = key ? defaultProfileName(key) : null;
    launchWith.textContent = name || (key ? 'Set up in Players and emulators' : 'Choose a platform');
    if (!name) launchWith.className = 'warn-text';

    tr.append(folder, platform, files, launchWith);
    tbody.appendChild(tr);
  }
}

$('addConfirmBtn').addEventListener('click', async () => {
  $('addConfirmBtn').disabled = true;
  try {
    const { added, missing, restored } = await api.addLibrary(state.add);
    const parts = [`Added ${added}`];
    if (restored) parts.push(`${restored} found again`);
    if (missing) parts.push(`${missing} no longer found`);
    toast(parts.join(', '));
    await loadLibrary();
    state.tab = state.add.library;
    state.homeFocus = null;
    renderHome();
    showScreen('home');
  } catch (err) {
    toast(message(err));
    $('addConfirmBtn').disabled = false;
  }
});

// ---------- Players and emulators ----------

async function openProfiles() {
  await loadProfiles();
  state.editing = null;
  renderProfiles();
  showScreen('profiles');
}

function renderProfiles() {
  const { profiles, defaults, presets, detected } = state.profiles;
  const list = $('profileList');
  list.replaceChildren();
  if (!profiles.length) {
    const p = document.createElement('p');
    p.className = 'muted';
    p.textContent = 'No players or emulators yet.';
    list.appendChild(p);
  }
  for (const profile of profiles) {
    const isDefaultFor = Object.keys(defaults).filter((k) => defaults[k] === profile.id);
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `profile-item${state.editing && state.editing.id === profile.id ? ' selected' : ''}`;
    btn.dataset.nav = '';
    const title = document.createElement('strong');
    title.textContent = profile.name;
    const sub = document.createElement('span');
    sub.className = 'muted small';
    const kind = profile.kind === 'player' ? 'Player' : 'Emulator';
    sub.textContent = [kind, profile.platforms.join(', '), isDefaultFor.length ? 'default' : ''].filter(Boolean).join(' · ');
    btn.append(title, sub);
    btn.addEventListener('click', () => edit(profile, false));
    list.appendChild(btn);
  }

  const presetList = $('presetList');
  presetList.replaceChildren();
  for (const preset of presets) {
    const found = detected.find((d) => d.id === preset.id);
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'preset-item';
    btn.dataset.nav = '';
    btn.textContent = found ? `${preset.name} (installed)` : preset.name;
    btn.addEventListener('click', () => edit({ ...preset, exe: found ? found.exe : '' }, true));
    presetList.appendChild(btn);
  }
}

const slug = (s) =>
  String(s)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48) || 'profile';

function uniqueId(base) {
  const taken = new Set(state.profiles.profiles.map((p) => p.id));
  if (!taken.has(base)) return base;
  for (let i = 2; ; i++) if (!taken.has(`${base}-${i}`)) return `${base}-${i}`;
}

function edit(profile, isNew) {
  state.editing = { ...profile, isNew };
  if (isNew) state.editing.id = uniqueId(slug(profile.id || profile.name));
  const { defaults } = state.profiles;
  $('pfName').value = profile.name || '';
  $('pfKind').value = profile.kind || 'emulator';
  $('pfExe').value = profile.exe || '';
  $('pfArgs').value = (profile.args || ['{file}']).join('\n');
  $('pfPlatforms').value = (profile.platforms || []).join(', ');
  // New profiles become the default where none is set yet.
  $('pfDefault').checked = isNew
    ? (profile.platforms || []).some((p) => !defaults[p])
    : Object.values(defaults).includes(profile.id);
  $('pfDeleteBtn').classList.toggle('hidden', isNew);
  $('pfErrors').textContent = '';
  $('profileForm').classList.remove('hidden');
  renderProfiles();
  MediaFocus.enter($('profileForm'));
}

$('newProfileBtn').addEventListener('click', () =>
  edit({ name: 'New profile', kind: 'emulator', exe: '', args: ['{file}'], platforms: [] }, true)
);

$('pfBrowseBtn').addEventListener('click', async () => {
  const exe = await api.pickProgram();
  if (exe) $('pfExe').value = exe;
});

async function saveProfilesData(data) {
  const result = await api.saveProfiles(data);
  if (!result.ok) {
    $('pfErrors').textContent = result.errors.join('. ');
    return false;
  }
  await loadProfiles();
  return true;
}

$('profileForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const editing = state.editing;
  const profile = {
    id: editing.id,
    name: $('pfName').value.trim(),
    kind: $('pfKind').value,
    exe: $('pfExe').value.trim(),
    args: $('pfArgs').value.split('\n').map((a) => a.trim()).filter(Boolean),
    platforms: $('pfPlatforms').value.split(',').map((p) => p.trim().toLowerCase()).filter(Boolean),
  };
  const others = state.profiles.profiles.filter((p) => p.id !== profile.id);
  const defaults = { ...state.profiles.defaults };
  for (const [key, id] of Object.entries(defaults)) {
    if (id === profile.id && (!$('pfDefault').checked || !profile.platforms.includes(key))) delete defaults[key];
  }
  if ($('pfDefault').checked) for (const p of profile.platforms) defaults[p] = profile.id;

  if (await saveProfilesData({ profiles: [...others, profile], defaults })) {
    toast(`Saved ${profile.name}`);
    edit(profile, false);
  }
});

$('pfDeleteBtn').addEventListener('click', async () => {
  const id = state.editing && state.editing.id;
  if (!id) return;
  const defaults = Object.fromEntries(Object.entries(state.profiles.defaults).filter(([, v]) => v !== id));
  if (await saveProfilesData({ profiles: state.profiles.profiles.filter((p) => p.id !== id), defaults })) {
    toast('Deleted');
    state.editing = null;
    $('profileForm').classList.add('hidden');
    renderProfiles();
    MediaFocus.enter($('profilesScreen'));
  }
});

// ---------- Customize art ----------

const SLOT_LABELS = { cover: 'Cover', banner: 'Banner', logo: 'Logo', screenshot: 'Screenshot' };
const SLOT_HINTS = {
  cover: 'Portrait box art, shown on game tiles',
  banner: 'Wide 16:9 art, shown on video tiles and behind the library',
  logo: 'Title logo with a transparent background, shown instead of the name',
  screenshot: 'A still from the game or video',
};

function sourceText(m) {
  if (!m) return 'Empty';
  if (m.source === 'user') return 'Set by you. Scrapes will not replace it.';
  return `From ${String(m.source).replace(/^scraper:/, '')}`;
}

function replaceEntry(updated) {
  state.entries = state.entries.map((e) => (e.id === updated.id ? updated : e));
}

function openCustomize(id) {
  const entry = state.entries.find((e) => e.id === id);
  if (!entry) return;
  state.customizing = id;
  renderCustomize();
  showScreen('customize');
}

async function changeArt(slot, call) {
  try {
    const updated = await call();
    if (!updated) return; // dialog cancelled
    replaceEntry(updated);
    renderCustomize(slot);
    renderHome();
  } catch (err) {
    toast(message(err));
  }
}

function renderCustomize(focusSlot) {
  const entry = state.entries.find((e) => e.id === state.customizing);
  if (!entry) return showScreen('home');
  $('customizeTitle').textContent = `Customize: ${entry.name}`;
  const grid = $('slotGrid');
  grid.replaceChildren();
  for (const slot of Object.keys(SLOT_LABELS)) {
    const card = document.createElement('section');
    card.className = `slot slot-${slot}`;
    card.setAttribute('aria-label', SLOT_LABELS[slot]);

    const head = document.createElement('div');
    head.className = 'slot-head';
    const title = document.createElement('h2');
    title.textContent = SLOT_LABELS[slot];
    const source = document.createElement('span');
    source.className = `small ${entry.media && entry.media[slot] && entry.media[slot].source === 'user' ? 'accent-text' : 'muted'}`;
    source.textContent = sourceText(entry.media && entry.media[slot]);
    head.append(title, source);

    const preview = document.createElement('div');
    preview.className = 'slot-preview';
    const url = artUrl(entry, slot);
    if (url) {
      const img = document.createElement('img');
      img.alt = `${SLOT_LABELS[slot]} for ${entry.name}`;
      img.src = url;
      preview.appendChild(img);
    } else {
      preview.textContent = 'Drop an image here';
    }
    // Dropping a file from Explorer onto the slot.
    preview.addEventListener('dragover', (e) => {
      e.preventDefault();
      preview.classList.add('drop');
    });
    preview.addEventListener('dragleave', () => preview.classList.remove('drop'));
    preview.addEventListener('drop', (e) => {
      e.preventDefault();
      preview.classList.remove('drop');
      const file = e.dataTransfer.files[0];
      if (file) changeArt(slot, () => api.setArtDropped(entry.id, slot, file));
    });

    const hint = document.createElement('p');
    hint.className = 'muted small';
    hint.textContent = SLOT_HINTS[slot];

    const buttons = document.createElement('div');
    buttons.className = 'slot-buttons';
    const fileBtn = document.createElement('button');
    fileBtn.type = 'button';
    fileBtn.dataset.nav = '';
    fileBtn.dataset.slot = slot;
    fileBtn.textContent = 'Choose file';
    fileBtn.addEventListener('click', () => changeArt(slot, () => api.setArtFile(entry.id, slot)));
    buttons.appendChild(fileBtn);
    if (url) {
      const clearBtn = document.createElement('button');
      clearBtn.type = 'button';
      clearBtn.dataset.nav = '';
      clearBtn.textContent = 'Clear';
      clearBtn.addEventListener('click', () => changeArt(slot, () => api.clearArt(entry.id, slot)));
      buttons.appendChild(clearBtn);
    }

    const urlForm = document.createElement('form');
    urlForm.className = 'slot-url';
    const urlInput = document.createElement('input');
    urlInput.type = 'url';
    urlInput.dataset.nav = '';
    urlInput.placeholder = 'https://…/image.jpg';
    urlInput.setAttribute('aria-label', `${SLOT_LABELS[slot]} image address`);
    const urlBtn = document.createElement('button');
    urlBtn.type = 'submit';
    urlBtn.dataset.nav = '';
    urlBtn.textContent = 'Use address';
    urlForm.append(urlInput, urlBtn);
    urlForm.addEventListener('submit', (e) => {
      e.preventDefault();
      if (urlInput.value.trim()) changeArt(slot, () => api.setArtUrl(entry.id, slot, urlInput.value));
    });

    card.append(head, preview, hint, buttons, urlForm);
    grid.appendChild(card);
  }
  if (focusSlot) {
    const btn = grid.querySelector(`[data-slot="${focusSlot}"]`);
    if (btn) MediaFocus.focus(btn);
  }
}

// ---------- Controller setup ----------

const BUTTON_NAMES = ['A', 'B', 'X', 'Y', 'LB', 'RB', 'LT', 'RT', 'Back', 'Start', 'Left stick', 'Right stick', 'D-pad up', 'D-pad down', 'D-pad left', 'D-pad right', 'Guide'];
const buttonName = (i) => BUTTON_NAMES[i] || `Button ${Number(i) + 1}`;
const ACTION_LABELS = {
  select: 'Select or launch',
  back: 'Back',
  customize: 'Customize art',
  quick: 'Quick actions',
  tabPrev: 'Previous tab',
  tabNext: 'Next tab',
  pageUp: 'Page up',
  pageDown: 'Page down',
  menu: 'Menu (Players and emulators)',
  up: 'Move up',
  down: 'Move down',
  left: 'Move left',
  right: 'Move right',
};

// The pad being set up: the first one connected.
function activePad() {
  return padControls ? padControls.pads()[0] || null : null;
}

function mappingFor(pad) {
  return (pad && state.controller.pads[pad.id]) || MediaPad.DEFAULT_MAPPING;
}

// Prompt glyphs that follow the active pad's mapping.
function padGlyphs() {
  const mapping = mappingFor(activePad());
  const nameOf = (action) => {
    const b = Object.keys(mapping).find((k) => mapping[k] === action);
    return b === undefined ? null : buttonName(b);
  };
  const prev = nameOf('tabPrev');
  const next = nameOf('tabNext');
  return {
    select: nameOf('select'),
    back: nameOf('back'),
    menu: nameOf('menu'),
    customize: nameOf('customize'),
    tabs: prev && next ? `${prev} / ${next}` : null,
  };
}

function applyController() {
  if (!padControls) return;
  padControls.configure({ deadZone: state.controller.deadZone, mappingFor });
  renderPrompts();
}

async function saveController() {
  state.controller = await api.saveController(state.controller);
  applyController();
}

async function openController() {
  state.controller = await api.controller();
  renderController();
  showScreen('controller');
}

function renderController(focusAction) {
  const pad = activePad();
  $('padName').textContent = pad
    ? `Setting up: ${pad.id}${pad.mapping === 'standard' ? '' : ' (not a standard layout: check every button)'}`
    : 'No controller connected. Connect one and press any button on it.';
  const mapping = mappingFor(pad);
  const tbody = $('mapRows');
  tbody.replaceChildren();
  for (const action of Object.keys(ACTION_LABELS)) {
    const tr = document.createElement('tr');
    if (state.capturing === action) tr.className = 'capturing';
    const label = document.createElement('td');
    label.textContent = ACTION_LABELS[action];
    const current = document.createElement('td');
    const b = Object.keys(mapping).find((k) => mapping[k] === action);
    const kbd = document.createElement('kbd');
    kbd.textContent = state.capturing === action ? 'Press a button…' : b === undefined ? 'None' : buttonName(b);
    current.appendChild(kbd);
    const change = document.createElement('td');
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.dataset.nav = '';
    btn.dataset.action = action;
    btn.textContent = 'Change';
    btn.disabled = !pad;
    btn.addEventListener('click', () => startCapture(action));
    change.appendChild(btn);
    tr.append(label, current, change);
    tbody.appendChild(tr);
  }
  $('deadZone').value = String(Math.round(state.controller.deadZone * 100));
  $('deadZoneValue').textContent = `${Math.round(state.controller.deadZone * 100)}%`;
  $('resetMapBtn').disabled = !pad || !state.controller.pads[pad.id];
  if (focusAction) {
    const btn = tbody.querySelector(`[data-action="${focusAction}"]`);
    if (btn) MediaFocus.focus(btn);
  }
}

let captureTimer = null;

function startCapture(action) {
  const pad = activePad();
  if (!pad) return;
  state.capturing = action;
  // Every pad button is captured, so cancelling is Esc or just waiting.
  $('captureStatus').textContent = `Press the button for "${ACTION_LABELS[action]}". Wait 8 seconds or press Esc to cancel.`;
  renderController(action);
  clearTimeout(captureTimer);
  captureTimer = setTimeout(cancelCapture, 8000);
  padControls.capture(async (button, fromPad) => {
    clearTimeout(captureTimer);
    state.capturing = null;
    const { mapping, swapped, unmapped } = MediaPad.assignButton(mappingFor(fromPad), action, button);
    state.controller.pads = { ...state.controller.pads, [fromPad.id]: mapping };
    if (swapped) $('captureStatus').textContent = `${buttonName(button)} is now ${ACTION_LABELS[action]}. ${ACTION_LABELS[swapped]} moved to its old button.`;
    else if (unmapped) $('captureStatus').textContent = `${buttonName(button)} is now ${ACTION_LABELS[action]}. ${ACTION_LABELS[unmapped]} has no button now.`;
    else $('captureStatus').textContent = `${buttonName(button)} is now ${ACTION_LABELS[action]}.`;
    await saveController();
    renderController(action);
  });
}

function cancelCapture() {
  clearTimeout(captureTimer);
  if (padControls) padControls.cancelCapture();
  const action = state.capturing;
  state.capturing = null;
  $('captureStatus').textContent = '';
  renderController(action);
}

$('deadZone').addEventListener('input', () => {
  state.controller.deadZone = Number($('deadZone').value) / 100;
  $('deadZoneValue').textContent = `${$('deadZone').value}%`;
});
$('deadZone').addEventListener('change', saveController);

$('resetMapBtn').addEventListener('click', async () => {
  const pad = activePad();
  if (!pad) return;
  const pads = { ...state.controller.pads };
  delete pads[pad.id];
  state.controller.pads = pads;
  await saveController();
  $('captureStatus').textContent = 'Back to the standard layout.';
  renderController();
});

window.addEventListener('gamepadconnected', () => {
  if (state.screen === 'controller') renderController();
  renderPrompts();
});

// ---------- Controller ----------

const TAB_ORDER = ['games', 'videos', 'all'];

function switchTab(step) {
  const i = TAB_ORDER.indexOf(state.tab);
  const next = TAB_ORDER[(i + step + TAB_ORDER.length) % TAB_ORDER.length];
  document.querySelector(`.tab[data-tab="${next}"]`).click();
}

// A on a dropdown steps to its next option (there is no pad-friendly way to
// open the native list); on anything else it is a click.
function padSelect() {
  const el = document.activeElement;
  if (!el || el === document.body) return MediaFocus.move('down');
  if (el.tagName === 'SELECT') {
    if (!el.options.length) return;
    el.selectedIndex = (el.selectedIndex + 1) % el.options.length;
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return;
  }
  if (el.tagName === 'INPUT' && el.type !== 'checkbox') return; // text needs a keyboard for now
  if (el.tagName === 'TEXTAREA') return;
  el.click();
}

function onPadAction(action) {
  switch (action) {
    case 'up':
    case 'down':
    case 'left':
    case 'right':
      MediaFocus.move(action);
      break;
    case 'pageUp':
    case 'pageDown':
      for (let i = 0; i < 3; i++) MediaFocus.move(action === 'pageUp' ? 'up' : 'down');
      break;
    case 'select':
      padSelect();
      break;
    case 'back':
      back();
      break;
    case 'tabPrev':
    case 'tabNext':
      if (state.screen === 'home') switchTab(action === 'tabNext' ? 1 : -1);
      break;
    case 'menu':
      if (state.screen === 'home') openProfiles();
      break;
    case 'customize': {
      const el = document.activeElement;
      if (state.screen === 'home' && el && el.classList.contains('tile')) openCustomize(el.dataset.id);
      break;
    }
    default:
      break; // quick (X) arrives with the scraper's quick actions
  }
}

const padControls = MediaPad.startPads({ onAction: onPadAction, onActive: () => setInput('pad') });

// ---------- Start ----------

(async () => {
  const platforms = await api.platforms();
  state.platforms = new Map(platforms.map((p) => [p.id, p.name]));
  const [controller] = await Promise.all([api.controller(), loadLibrary(), loadProfiles()]);
  state.controller = controller;
  applyController();
  if (!state.entries.some((e) => e.library === 'games') && state.entries.some((e) => e.library === 'videos')) {
    state.tab = 'videos';
  }
  renderHome();
  const running = await api.running();
  if (running.length) {
    const r = running[0];
    const entry = state.entries.find((e) => e.id === r.id);
    const profile = entry ? profileFor(entry) : null;
    state.running = { id: r.id, name: r.name, with: profile ? profile.name : '', startedAt: r.startedAt };
    showRunning();
  } else {
    showScreen('home');
  }
})();
