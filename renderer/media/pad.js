// Controller input for Media Mode. Reads pads through the browser Gamepad
// API and turns them into named actions (up, down, left, right, select,
// back, tabPrev, tabNext, menu, pageUp, pageDown, quick, customize).
// createPadReader() holds no browser state, so it also runs under node:test;
// startPads() is the browser loop around it.
(() => {
  // The Gamepad API's standard mapping (Xbox layout). Pads Chromium doesn't
  // recognize report other indices; remapping them comes with the Controller
  // setup screen.
  const DEFAULT_MAPPING = {
    0: 'select', // A
    1: 'back', // B
    2: 'quick', // X
    3: 'customize', // Y
    4: 'tabPrev', // LB
    5: 'tabNext', // RB
    6: 'pageUp', // LT
    7: 'pageDown', // RT
    9: 'menu', // Start
    12: 'up',
    13: 'down',
    14: 'left',
    15: 'right',
  };
  const DEFAULT_DEAD_ZONE = 0.35;
  const TRIGGER_PRESS = 0.5; // analogue triggers count as pressed past this
  const REPEAT_DELAY_MS = 400;
  const REPEAT_RATE_MS = 80;
  const REPEATS = new Set(['up', 'down', 'left', 'right', 'pageUp', 'pageDown']);

  // The left stick as a direction, or null inside the dead zone. The larger
  // axis wins, so a diagonal push moves one way, not two.
  function stickDirection(x, y, deadZone) {
    if (Math.hypot(x, y) < deadZone) return null;
    if (Math.abs(x) >= Math.abs(y)) return x < 0 ? 'left' : 'right';
    return y < 0 ? 'up' : 'down';
  }

  // Which actions a pad snapshot is holding. `pad` is shaped like a Gamepad:
  // { buttons: [{ pressed, value }], axes: [x, y, …] }.
  function heldActions(pad, mapping, deadZone) {
    const held = new Set();
    (pad.buttons || []).forEach((b, i) => {
      const action = mapping[i];
      if (!action || !b) return;
      if (b.pressed || (typeof b.value === 'number' && b.value >= TRIGGER_PRESS)) held.add(action);
    });
    const axes = pad.axes || [];
    const dir = stickDirection(axes[0] || 0, axes[1] || 0, deadZone);
    if (dir) held.add(dir);
    return held;
  }

  // Edge-triggered actions with key-repeat for movement: an action fires
  // when pressed, then (if it repeats) after REPEAT_DELAY_MS and every
  // REPEAT_RATE_MS while held. Several pads act as one.
  function createPadReader({ mapping = DEFAULT_MAPPING, deadZone = DEFAULT_DEAD_ZONE } = {}) {
    const holding = new Map(); // action -> time it should fire next

    function update(pads, now) {
      const held = new Set();
      for (const pad of pads) {
        if (pad && pad.connected !== false) for (const a of heldActions(pad, mapping, deadZone)) held.add(a);
      }
      const fired = [];
      for (const action of held) {
        if (!holding.has(action)) {
          fired.push(action);
          holding.set(action, REPEATS.has(action) ? now + REPEAT_DELAY_MS : Infinity);
        } else if (now >= holding.get(action)) {
          fired.push(action);
          holding.set(action, now + REPEAT_RATE_MS);
        }
      }
      for (const action of [...holding.keys()]) if (!held.has(action)) holding.delete(action);
      return fired;
    }

    // Forget what is held, so buttons still down when input resumes don't
    // fire (the A that closed a game must not also launch something).
    // Held buttons are swallowed until released.
    function reset(pads) {
      holding.clear();
      for (const pad of pads || []) {
        if (pad) for (const a of heldActions(pad, mapping, deadZone)) holding.set(a, Infinity);
      }
    }

    return { update, reset };
  }

  // Browser loop: polls every animation frame while any pad is connected
  // and calls onAction(action). onActive() fires on the first pad input
  // after other input, so the prompts can switch to controller glyphs.
  function startPads({ onAction, onActive, focusGraceMs = 500 }) {
    const reader = createPadReader();
    let running = false;
    let ignoreUntil = 0;
    const pads = () => [...navigator.getGamepads()].filter(Boolean);

    // Chromium only reports pads to a focused page; on coming back, ignore
    // whatever is still held for a moment.
    const regain = () => {
      ignoreUntil = performance.now() + focusGraceMs;
      reader.reset(pads());
    };
    window.addEventListener('focus', regain);
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) regain();
    });

    function frame(now) {
      const list = pads();
      if (!list.length) {
        running = false;
        return;
      }
      if (now < ignoreUntil || !document.hasFocus()) {
        reader.reset(list);
      } else {
        const actions = reader.update(list, now);
        if (actions.length) onActive();
        for (const a of actions) onAction(a);
      }
      requestAnimationFrame(frame);
    }
    const start = () => {
      if (running) return;
      running = true;
      reader.reset(pads());
      requestAnimationFrame(frame);
    };
    window.addEventListener('gamepadconnected', start);
    if (pads().length) start();
  }

  const api = { createPadReader, stickDirection, heldActions, startPads, DEFAULT_MAPPING, DEFAULT_DEAD_ZONE, REPEAT_DELAY_MS, REPEAT_RATE_MS };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else window.MediaPad = api;
})();
