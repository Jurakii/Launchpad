// Spatial focus for Media Mode: the arrow keys (and, later, a controller's
// D-pad and stick) move focus to the nearest [data-nav] element in that
// direction. Everything focusable stays a real button, input or select, so
// Enter, Space and Tab keep their usual meaning.
(() => {
  const visible = (el) => {
    if (el.disabled) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && !el.closest('.hidden');
  };

  const candidates = () => [...document.querySelectorAll('[data-nav]')].filter(visible);

  // Distance along the direction, plus a penalty for drifting sideways, so
  // moving down a column prefers the tile right below over a closer one off
  // to the side.
  function score(from, to, dir) {
    const fx = from.left + from.width / 2;
    const fy = from.top + from.height / 2;
    const tx = to.left + to.width / 2;
    const ty = to.top + to.height / 2;
    const dx = tx - fx;
    const dy = ty - fy;
    const along = { left: -dx, right: dx, up: -dy, down: dy }[dir];
    if (along <= 1) return Infinity;
    const across = dir === 'left' || dir === 'right' ? Math.abs(dy) : Math.abs(dx);
    return along + across * 2;
  }

  function move(dir) {
    const items = candidates();
    if (!items.length) return false;
    const current = document.activeElement;
    if (!current || !items.includes(current)) {
      focus(items[0]);
      return true;
    }
    const from = current.getBoundingClientRect();
    let best = null;
    let bestScore = Infinity;
    for (const el of items) {
      if (el === current) continue;
      const s = score(from, el.getBoundingClientRect(), dir);
      if (s < bestScore) {
        best = el;
        bestScore = s;
      }
    }
    if (best) focus(best);
    return !!best;
  }

  function focus(el) {
    el.focus({ preventScroll: true });
    el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' });
  }

  // Focuses the first [data-nav] inside root (or `preferred` when it is
  // still on screen).
  function enter(root, preferred) {
    if (preferred && preferred.isConnected && visible(preferred) && root.contains(preferred)) {
      focus(preferred);
      return;
    }
    const first = [...root.querySelectorAll('[data-nav]')].find(visible);
    if (first) focus(first);
  }

  // Text fields keep left/right for the cursor; selects keep up/down for
  // their options.
  function keepsKey(el, dir) {
    if (!el) return false;
    const tag = el.tagName;
    const text = tag === 'TEXTAREA' || (tag === 'INPUT' && !['checkbox', 'radio', 'button'].includes(el.type));
    if (text && (dir === 'left' || dir === 'right')) return true;
    if (tag === 'TEXTAREA' && el.value.includes('\n')) return true;
    return tag === 'SELECT' && (dir === 'up' || dir === 'down');
  }

  const KEY_DIRS = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };

  document.addEventListener('keydown', (e) => {
    const dir = KEY_DIRS[e.key];
    if (!dir || e.altKey || e.ctrlKey || e.metaKey) return;
    if (keepsKey(document.activeElement, dir)) return;
    if (move(dir)) e.preventDefault();
  });

  window.MediaFocus = { move, enter, focus };
})();
