// The bottom prompt bar: which actions each screen offers, shown with the
// keys or the controller buttons depending on what was used last.
(() => {
  const GLYPHS = {
    keys: { select: 'Enter', move: 'Arrows', back: 'Esc', customize: 'C' },
    pad: { select: 'A', move: 'D-pad', back: 'B', tabs: 'LB / RB', menu: 'Start', customize: 'Y' },
  };

  // [action, label]; an action with no glyph for the current input is left out.
  const SCREENS = {
    home: [
      ['select', 'Launch'],
      ['move', 'Move'],
      ['customize', 'Customize art'],
      ['tabs', 'Switch tab'],
      ['menu', 'Players and emulators'],
      ['back', 'Exit Media Mode'],
    ],
    add: [['select', 'Select'], ['move', 'Move'], ['back', 'Back']],
    profiles: [['select', 'Select'], ['move', 'Move'], ['back', 'Back']],
    customize: [['select', 'Select'], ['move', 'Move'], ['back', 'Back']],
    controller: [['select', 'Change'], ['move', 'Move'], ['back', 'Back']],
    running: [['select', 'Close it'], ['back', 'Back to library']],
  };

  // padGlyphs: the buttons the active pad's mapping really uses (null where
  // an action has none), so a remapped pad shows its own buttons.
  function render(footer, screen, input, padGlyphs = {}) {
    let glyphs = GLYPHS.keys;
    if (input === 'pad') {
      glyphs = { ...GLYPHS.pad };
      for (const [action, glyph] of Object.entries(padGlyphs)) glyphs[action] = glyph;
    }
    footer.replaceChildren();
    for (const [action, label] of SCREENS[screen] || []) {
      const glyph = glyphs[action];
      if (!glyph) continue;
      const item = document.createElement('span');
      const kbd = document.createElement('kbd');
      kbd.textContent = glyph;
      item.append(kbd, ` ${label}`);
      footer.appendChild(item);
    }
  }

  window.MediaPrompts = { render };
})();
