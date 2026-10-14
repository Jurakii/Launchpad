// Geometry for the layout strip that drops down from the top of a display
// while a window is being dragged. Main hit-tests the cursor against these
// rects and the overlay draws exactly the same rects, so what the user sees
// highlighted is what they'll get. Everything is in DIP screen coordinates.

const zones = require('./zones');

const PAD = 14; // strip edge to thumbnails
const THUMB_GAP = 14; // between thumbnails
const MAX_THUMB_W = 96;
const MIN_THUMB_W = 40;
const LABEL_H = 20; // layout name under each thumbnail

// How close to the top of the work area the cursor has to get before the
// strip appears, and how far outside the strip it can wander before the
// strip goes away again. The margin stops it flickering at the edges.
const TRIGGER_DIP = 48;
const STAY_MARGIN_DIP = 48;

const contains = (r, p) => p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height;

// One thumbnail per layout, centred along the top of the work area.
// Thumbnails share the display's aspect ratio so zones look like the result,
// and shrink when many layouts have to fit on a narrow display.
function computeStrip(layouts, workArea) {
  const n = layouts.length;
  const fitW = Math.floor((workArea.width - 2 * PAD - (n - 1) * THUMB_GAP) / n);
  const thumbW = Math.max(MIN_THUMB_W, Math.min(MAX_THUMB_W, fitW));
  const thumbH = Math.round((thumbW * workArea.height) / workArea.width);
  const width = 2 * PAD + n * thumbW + (n - 1) * THUMB_GAP;
  const height = PAD + thumbH + LABEL_H + PAD / 2;
  const x = workArea.x + Math.round((workArea.width - width) / 2);
  const y = workArea.y;

  const items = layouts.map((layout, i) => {
    const thumb = { x: x + PAD + i * (thumbW + THUMB_GAP), y: y + PAD, width: thumbW, height: thumbH };
    return {
      layoutId: layout.id,
      name: layout.name,
      thumb,
      // No gap: the zones tile the whole thumbnail, so the cursor is always
      // over some zone while it's over a thumbnail (the overlay draws them
      // slightly inset to show the divisions).
      zones: layout.zones.map((z) => ({ zoneId: z.id, rect: zones.zoneRect(layout, z, thumb, 0) })),
    };
  });
  return { rect: { x, y, width, height }, items };
}

// { layoutId, zoneId } under point p, or null.
function zoneAt(strip, p) {
  for (const item of strip.items) {
    if (!contains(item.thumb, p)) continue;
    const zone = item.zones.find((z) => contains(z.rect, p));
    return zone ? { layoutId: item.layoutId, zoneId: zone.zoneId } : null;
  }
  return null;
}

// Whether the strip should be showing for cursor p. It appears once the
// cursor nears the top of the work area (including above it, e.g. over a
// Windows taskbar docked at the top), and stays while the cursor is over or
// near the strip so the user can move down to the thumbnails.
function shouldShow(strip, workArea, p, wasShowing) {
  if (p.y < workArea.y + TRIGGER_DIP) return true;
  // Being where the strip is drawn counts too: a quick flick to the top and
  // back down onto a thumbnail can pass through the band between two polls.
  if (contains(strip.rect, p)) return true;
  if (!wasShowing) return false;
  const r = strip.rect;
  const stay = { x: r.x - STAY_MARGIN_DIP, y: r.y, width: r.width + 2 * STAY_MARGIN_DIP, height: r.height + STAY_MARGIN_DIP };
  return contains(stay, p);
}

module.exports = { computeStrip, zoneAt, shouldShow, TRIGGER_DIP, STAY_MARGIN_DIP };
