const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const theme = require('../renderer/media/theme');

test('every colour token in media.css is editable, with the same defaults for dark and light', () => {
  const css = fs.readFileSync(path.join(__dirname, '..', 'renderer', 'media', 'media.css'), 'utf-8');
  const block = (selector) => {
    const start = css.indexOf(`${selector} {`);
    return css.slice(start, css.indexOf('}', start));
  };
  const read = (text) => Object.fromEntries([...text.matchAll(/(--[a-z-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2].toLowerCase()]));
  const dark = read(block(':root'));
  const light = read(block(":root[data-theme='light']"));
  assert.deepStrictEqual(Object.keys(dark).sort(), theme.TOKENS.map((t) => t.token).sort());
  for (const t of theme.TOKENS) {
    assert.strictEqual(t.dark, dark[t.token], `${t.token} dark`);
    assert.strictEqual(t.light, light[t.token], `${t.token} light`);
  }
});

test('media.css uses no colour outside the tokens', () => {
  const css = fs.readFileSync(path.join(__dirname, '..', 'renderer', 'media', 'media.css'), 'utf-8');
  const rules = css.replace(/:root(\[data-theme='light'\])? \{[^}]*\}/g, '').replace(/\/\*[\s\S]*?\*\//g, '');
  assert.deepStrictEqual(rules.match(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/gi), null);
});

test('clean keeps only known tokens with #rrggbb values', () => {
  assert.deepStrictEqual(theme.clean({ '--text': '#ABCDEF', '--bg': 'red', '--evil': '#000000', '--panel': '#123; color: x' }), {
    '--text': '#abcdef',
  });
  assert.deepStrictEqual(theme.clean(null), {});
  assert.deepStrictEqual(theme.clean(['#000000']), {});
});

test('effective: overrides win, the main window accent stands in for an unset Media accent', () => {
  const dark = theme.effective({}, 'dark', null);
  assert.strictEqual(dark['--bg'], '#16181d');
  assert.strictEqual(theme.effective({}, 'light', null)['--bg'], '#f2f3f6');
  assert.strictEqual(theme.effective({}, 'dark', '#ff0000')['--accent'], '#ff0000');
  assert.strictEqual(theme.effective({ '--accent': '#00ff00' }, 'dark', '#ff0000')['--accent'], '#00ff00');
});

test('contrast follows WCAG and the defaults pass AA; a bad choice is reported', () => {
  assert.strictEqual(Math.round(theme.contrast('#000000', '#ffffff')), 21);
  assert.strictEqual(theme.contrast('#777777', '#777777'), 1);
  assert.deepStrictEqual(theme.lowContrast(theme.effective({}, 'dark')), []);
  assert.deepStrictEqual(theme.lowContrast(theme.effective({}, 'light')), []);
  const low = theme.lowContrast(theme.effective({ '--text': '#20232a' }, 'dark'));
  assert.deepStrictEqual(low.map((p) => p.label), ['Text on panels', 'Text on the background']);
});
