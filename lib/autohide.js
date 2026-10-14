// When an auto-hiding organizer panel comes down and goes away again, decided
// from cursor samples: nothing of Launchpad's gets mouse events while the
// cursor is over other apps, so main polls the cursor and feeds it here.
// Pure, so it's testable without Electron. Everything is in DIP.

// The cursor has to rest against the top edge this long before the panel
// comes down. Without the wait, flicking up to a maximized window's title
// bar or close button would put the panel in the way of the click.
const REVEAL_DELAY_MS = 250;
// How far below or beside the panel the cursor can go, and for how long,
// before the panel goes away again.
const STAY_MARGIN_DIP = 16;
const HIDE_DELAY_MS = 500;
// The band along the very top of the display that counts as the edge.
const EDGE_DIP = 2;

const contains = (r, p) => p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height;

const initial = (revealed = false) => ({ revealed, since: null });

// One cursor sample. state is what the last call returned; since is when the
// cursor started doing what will change the panel's state (resting at the
// edge, or being away). bounds: the display's bounds. panel: the panel's
// rect. canReveal false (a window is being dragged, or a fullscreen app has
// the display) keeps a hidden panel hidden.
function step(state, { cursor: p, bounds, panel, now, canReveal }) {
  if (!state.revealed) {
    const edge = { x: bounds.x, y: bounds.y, width: bounds.width, height: EDGE_DIP };
    if (!canReveal || !contains(edge, p)) return initial(false);
    const since = state.since === null ? now : state.since;
    return now - since >= REVEAL_DELAY_MS ? initial(true) : { revealed: false, since };
  }
  const stay = {
    x: panel.x - STAY_MARGIN_DIP,
    y: panel.y,
    width: panel.width + 2 * STAY_MARGIN_DIP,
    height: panel.height + STAY_MARGIN_DIP,
  };
  if (contains(stay, p)) return initial(true);
  const since = state.since === null ? now : state.since;
  return now - since >= HIDE_DELAY_MS ? initial(false) : { revealed: true, since };
}

module.exports = { initial, step, REVEAL_DELAY_MS, HIDE_DELAY_MS, STAY_MARGIN_DIP, EDGE_DIP };
