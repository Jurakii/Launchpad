// Media's colours: every colour the page shows is one of these tokens on
// :root (media.css). The user's overrides live in localStorage under
// launchpad-media-colors (a JSON map of token -> #rrggbb) and are applied
// here, before first paint: this file is loaded from <head>, since the CSP
// blocks inline scripts. Theme (light/dark) still follows the main window's
// launchpad-theme. Media's accent is its own token, so Media can be themed
// apart from the rest of Launchpad; with no override it follows the main
// window's launchpad-accent like before.
//
// Also loaded by test/media-theme.test.js (module.exports at the end).
(() => {
  const STORAGE_KEY = 'launchpad-media-colors';

  // token, label, and the defaults media.css uses for dark and light.
  const TOKENS = [
    { token: '--bg', label: 'Background', dark: '#16181d', light: '#f2f3f6' },
    { token: '--panel', label: 'Panels and tiles', dark: '#1e2128', light: '#ffffff' },
    { token: '--raised', label: 'Buttons and fields', dark: '#262a33', light: '#e6e9ef' },
    { token: '--border', label: 'Borders', dark: '#2b2f38', light: '#dde1e8' },
    { token: '--text', label: 'Text', dark: '#e8e9ec', light: '#1b1d22' },
    { token: '--muted', label: 'Secondary text', dark: '#a3a9b5', light: '#5d6573' },
    { token: '--accent', label: 'Accent', dark: '#5b8cff', light: '#3b6fe0' },
    { token: '--on-accent', label: 'Text on accent', dark: '#0b1020', light: '#ffffff' },
    { token: '--warn', label: 'Warnings', dark: '#f5c26b', light: '#8a5a00' },
    { token: '--danger', label: 'Delete and errors', dark: '#ff5b6b', light: '#c42a3f' },
    { token: '--shadow', label: 'Shadows', dark: '#000000', light: '#000000' },
  ];

  // Pairs checked for contrast (text, background, what to call it).
  const CONTRAST_PAIRS = [
    ['--text', '--panel', 'Text on panels'],
    ['--text', '--bg', 'Text on the background'],
    ['--muted', '--panel', 'Secondary text on panels'],
    ['--on-accent', '--accent', 'Text on accent'],
  ];

  const HEX_RE = /^#[0-9a-f]{6}$/i;

  function luminance(hex) {
    const n = parseInt(hex.slice(1), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => {
      const c = v / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }

  // WCAG contrast ratio of two #rrggbb colours (1 to 21).
  function contrast(a, b) {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  }

  // Only known tokens with plain #rrggbb values survive, so a hand-edited
  // or stale entry can't inject anything into the page's styles.
  function clean(map) {
    const out = {};
    if (!map || typeof map !== 'object' || Array.isArray(map)) return out;
    for (const { token } of TOKENS) if (typeof map[token] === 'string' && HEX_RE.test(map[token])) out[token] = map[token].toLowerCase();
    return out;
  }

  // The value each token has right now: the override, else the default for
  // the theme (with the main window's accent standing in for Media's).
  function effective(overrides, theme, sharedAccent) {
    const out = {};
    for (const t of TOKENS) {
      let value = theme === 'light' ? t.light : t.dark;
      if (t.token === '--accent' && sharedAccent && HEX_RE.test(sharedAccent)) value = sharedAccent.toLowerCase();
      out[t.token] = overrides[t.token] || value;
    }
    return out;
  }

  // [{ label, ratio }] for every pair under WCAG AA (4.5:1).
  function lowContrast(values) {
    return CONTRAST_PAIRS.map(([fg, bg, label]) => ({ label, ratio: contrast(values[fg], values[bg]) })).filter((p) => p.ratio < 4.5);
  }

  const api = { STORAGE_KEY, TOKENS, CONTRAST_PAIRS, clean, contrast, effective, lowContrast };

  if (typeof document !== 'undefined') {
    const storage = {
      get(key) {
        try {
          return localStorage.getItem(key);
        } catch {
          return null; // storage blocked: defaults
        }
      },
      set(key, value) {
        try {
          if (value === null) localStorage.removeItem(key);
          else localStorage.setItem(key, value);
        } catch {}
      },
    };

    const theme = () => (storage.get('launchpad-theme') === 'light' ? 'light' : 'dark');

    function loadOverrides() {
      try {
        return clean(JSON.parse(storage.get(STORAGE_KEY) || '{}'));
      } catch {
        return {};
      }
    }

    function apply(overrides = loadOverrides()) {
      const root = document.documentElement;
      if (theme() === 'light') root.setAttribute('data-theme', 'light');
      else root.removeAttribute('data-theme');
      const sharedAccent = storage.get('launchpad-accent');
      for (const { token } of TOKENS) root.style.removeProperty(token);
      if (sharedAccent) root.style.setProperty('--accent', sharedAccent);
      for (const [token, value] of Object.entries(overrides)) root.style.setProperty(token, value);
    }

    function save(overrides) {
      const c = clean(overrides);
      storage.set(STORAGE_KEY, Object.keys(c).length ? JSON.stringify(c) : null);
      apply(c);
      return c;
    }

    Object.assign(api, {
      load: loadOverrides,
      save,
      apply,
      current: () => effective(loadOverrides(), theme(), storage.get('launchpad-accent')),
    });

    apply();
    // The main window changing theme or accent while Media is open.
    window.addEventListener('storage', () => apply());
    window.MediaTheme = api;
  }

  if (typeof module !== 'undefined') module.exports = api;
})();
