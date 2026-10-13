// Pure helpers for finding and decoding a website's own icon - no Electron
// or network here, so they're unit-testable. lib/icons.js does the fetching.

// Accepts what people type into the URL field ("example.com/page" or a
// full URL). Only http(s) - other schemes (steam://, mailto:) have no
// favicon to fetch.
function normalizeUrl(input) {
  const text = String(input || '').trim();
  if (!text) return null;
  // "localhost:3000" is a host and port, not a scheme.
  const hasScheme = /^[a-z][a-z0-9+.-]*:(?!\d)/i.test(text);
  try {
    const url = new URL(hasScheme ? text : `https://${text}`);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url : null;
  } catch {
    return null;
  }
}

const ENTITIES = { amp: '&', quot: '"', apos: "'", lt: '<', gt: '>' };

function decodeEntities(text) {
  return text.replace(/&(#x[0-9a-f]+|#\d+|\w+);/gi, (m, code) => {
    if (code[0] === '#') {
      const n = code[1] === 'x' || code[1] === 'X' ? parseInt(code.slice(2), 16) : parseInt(code.slice(1), 10);
      return Number.isFinite(n) ? String.fromCodePoint(n) : m;
    }
    return ENTITIES[code.toLowerCase()] ?? m;
  });
}

function parseAttributes(tag) {
  const attrs = {};
  const re = /([^\s=/>"']+)\s*(?:=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
  let m;
  re.exec(tag); // skip the tag name
  while ((m = re.exec(tag))) {
    attrs[m[1].toLowerCase()] = decodeEntities(m[2] ?? m[3] ?? m[4] ?? '');
  }
  return attrs;
}

// Largest edge from a sizes="16x16 32x32" attribute, or 0 if not given.
function largestDeclaredSize(sizes) {
  let best = 0;
  for (const token of String(sizes || '').split(/\s+/)) {
    const m = /^(\d+)x(\d+)$/i.exec(token);
    if (m) best = Math.max(best, Number(m[1]), Number(m[2]));
  }
  return best;
}

// Icon URLs declared in a page's <link> tags, best first. SVG icons are
// skipped - nativeImage can't rasterize them in the main process - and so
// is Safari's monochrome mask-icon (its rel token isn't "icon").
function findIconLinks(html, pageUrl) {
  let base = pageUrl;
  const baseTag = /<base\b[^>]*>/i.exec(html);
  if (baseTag) {
    const href = parseAttributes(baseTag[0]).href;
    try {
      if (href) base = new URL(href, pageUrl).href;
    } catch {}
  }

  const found = [];
  const seen = new Set();
  for (const [tag] of html.matchAll(/<link\b[^>]*>/gi)) {
    const attrs = parseAttributes(tag);
    const rels = String(attrs.rel || '').toLowerCase().split(/\s+/);
    const touch = rels.includes('apple-touch-icon') || rels.includes('apple-touch-icon-precomposed');
    if (!touch && !rels.includes('icon')) continue;
    if (!attrs.href) continue;

    let url;
    try {
      url = new URL(attrs.href.trim(), base);
    } catch {
      continue;
    }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') continue;
    if (/svg/i.test(attrs.type || '') || /\.svgz?$/i.test(url.pathname)) continue;
    if (seen.has(url.href)) continue;
    seen.add(url.href);

    // Touch icons are almost always 180px PNGs even when sizes is omitted.
    // A plain rel=icon with no sizes is usually a 16/32px favicon.
    const size = largestDeclaredSize(attrs.sizes) || (touch ? 180 : 32);
    found.push({ url: url.href, size });
  }
  return found.sort((a, b) => b.size - a.size).map((c) => c.url);
}

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

function isPng(buf) {
  return buf.length >= 8 && buf.subarray(0, 8).equals(PNG_SIGNATURE);
}

function isJpeg(buf) {
  return buf.length >= 3 && buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff;
}

function isIco(buf) {
  return buf.length >= 6 && buf.readUInt16LE(0) === 0 && buf.readUInt16LE(2) === 1 && buf.readUInt16LE(4) > 0;
}

// Decodes one BMP-style ICO image (a DIB with no file header, height
// doubled to cover the 1-bit AND transparency mask that follows the
// pixels). Returns premultiplied BGRA rows top-down - what
// nativeImage.createFromBitmap expects on Windows - or null if unsupported.
function decodeIcoDib(data) {
  if (data.length < 40) return null;
  const headerSize = data.readUInt32LE(0);
  const width = data.readInt32LE(4);
  const height = Math.floor(Math.abs(data.readInt32LE(8)) / 2);
  const bpp = data.readUInt16LE(14);
  const compression = data.readUInt32LE(16);
  if (compression !== 0 || ![1, 4, 8, 24, 32].includes(bpp)) return null; // BI_RGB only
  if (width <= 0 || height <= 0 || width > 1024 || height > 1024) return null;

  const paletteCount = bpp <= 8 ? data.readUInt32LE(32) || 1 << bpp : 0;
  const pixelStart = headerSize + paletteCount * 4;
  const rowSize = Math.ceil((width * bpp) / 32) * 4;
  const maskStart = pixelStart + rowSize * height;
  const maskRowSize = Math.ceil(width / 32) * 4;
  if (maskStart > data.length) return null;
  const hasMask = maskStart + maskRowSize * height <= data.length;

  const out = Buffer.alloc(width * height * 4);
  let anyAlpha = false;
  for (let y = 0; y < height; y++) {
    const row = pixelStart + (height - 1 - y) * rowSize; // stored bottom-up
    for (let x = 0; x < width; x++) {
      const o = (y * width + x) * 4;
      if (bpp === 32 || bpp === 24) {
        const p = row + x * (bpp / 8);
        out[o] = data[p];
        out[o + 1] = data[p + 1];
        out[o + 2] = data[p + 2];
        out[o + 3] = bpp === 32 ? data[p + 3] : 255;
      } else {
        let index;
        if (bpp === 8) index = data[row + x];
        else if (bpp === 4) index = (data[row + (x >> 1)] >> (x & 1 ? 0 : 4)) & 0x0f;
        else index = (data[row + (x >> 3)] >> (7 - (x & 7))) & 1;
        const p = headerSize + Math.min(index, paletteCount - 1) * 4;
        out[o] = data[p];
        out[o + 1] = data[p + 1];
        out[o + 2] = data[p + 2];
        out[o + 3] = 255;
      }
      if (out[o + 3]) anyAlpha = true;
    }
  }

  // Below 32bpp - or old 32bpp icons that left the alpha channel empty -
  // transparency comes from the AND mask instead.
  if (hasMask && (bpp < 32 || !anyAlpha)) {
    for (let y = 0; y < height; y++) {
      const row = maskStart + (height - 1 - y) * maskRowSize;
      for (let x = 0; x < width; x++) {
        const transparent = (data[row + (x >> 3)] >> (7 - (x & 7))) & 1;
        out[(y * width + x) * 4 + 3] = transparent ? 0 : 255;
      }
    }
  }

  for (let o = 0; o < out.length; o += 4) {
    const a = out[o + 3];
    if (a === 255) continue;
    out[o] = Math.round((out[o] * a) / 255);
    out[o + 1] = Math.round((out[o + 1] * a) / 255);
    out[o + 2] = Math.round((out[o + 2] * a) / 255);
  }
  return { bitmap: out, width, height };
}

// Picks the largest image in an .ico file. Returns { png } when that image
// is embedded PNG (common for 256px entries), { bitmap, width, height } for
// a BMP-style entry, or null if nothing in it decodes.
function decodeIco(buf) {
  if (!isIco(buf)) return null;
  const entries = [];
  const count = buf.readUInt16LE(4);
  for (let i = 0; i < count; i++) {
    const o = 6 + i * 16;
    if (o + 16 > buf.length) break;
    const size = buf.readUInt32LE(o + 8);
    const offset = buf.readUInt32LE(o + 12);
    if (!size || offset + size > buf.length) continue;
    entries.push({ width: buf[o] || 256, bpp: buf.readUInt16LE(o + 6), data: buf.subarray(offset, offset + size) });
  }
  entries.sort((a, b) => b.width - a.width || b.bpp - a.bpp);
  for (const entry of entries) {
    if (isPng(entry.data)) return { png: entry.data };
    const decoded = decodeIcoDib(entry.data);
    if (decoded) return decoded;
  }
  return null;
}

module.exports = { normalizeUrl, findIconLinks, isPng, isJpeg, isIco, decodeIco };
