// Artwork for library entries: one image per slot in media\<id>\, resized
// on the way in so a 4K banner doesn't cost 4K on every tile. Each slot
// remembers who set it ('user' or 'scraper:<source>') so a scrape never
// replaces art the user picked. Images reach the Media Mode window through
// the lpmedia: protocol (art-protocol.js), never as data URLs.
const fs = require('fs');
const path = require('path');
const electron = require('electron');
const library = require('./library');
const favicon = require('../favicon');

// Largest size kept per slot (fit inside, never enlarged) and the format:
// logos keep transparency, the rest are photos.
const SLOTS = {
  cover: { w: 600, h: 900, ext: 'jpg' },
  banner: { w: 1920, h: 1080, ext: 'jpg' },
  logo: { w: 800, h: 400, ext: 'png' },
  screenshot: { w: 1280, h: 720, ext: 'jpg' },
};
const IMAGE_SLOTS = Object.keys(SLOTS);
const MAX_FILE_BYTES = 25 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES = 15 * 1024 * 1024;
const DOWNLOAD_TIMEOUT_MS = 20000;
const JPEG_QUALITY = 88;

function checkSlot(slot) {
  if (!IMAGE_SLOTS.includes(slot)) throw new Error(`Unknown art slot: ${slot}`);
  return SLOTS[slot];
}

const fileNameFor = (slot) => `${slot}.${checkSlot(slot).ext}`;

// The size that fits (w, h) inside (maxW, maxH) keeping its shape; never larger.
function fitSize(w, h, maxW, maxH) {
  const scale = Math.min(1, maxW / w, maxH / h);
  return { width: Math.max(1, Math.round(w * scale)), height: Math.max(1, Math.round(h * scale)) };
}

function artPath(id, slot) {
  return path.join(library.mediaDirFor(id), fileNameFor(slot));
}

// PNG or JPEG only: nativeImage can't decode anything else from a buffer.
function decode(buf) {
  if (!favicon.isPng(buf) && !favicon.isJpeg(buf)) throw new Error('Use a PNG or JPEG image');
  const img = electron.nativeImage.createFromBuffer(buf);
  if (img.isEmpty()) throw new Error("That image couldn't be read");
  return img;
}

// Writes next to the target and renames over it, so a tile never loads a
// half-written image.
function writeReplacing(file, buf) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.tmp`;
  fs.writeFileSync(tmp, buf);
  try {
    fs.renameSync(tmp, file);
  } catch (err) {
    fs.rmSync(tmp, { force: true });
    throw err;
  }
}

// Resizes and stores an image for one slot. Returns the slot's new value
// for entry.media: { path, source, updatedAt }, path relative to the
// entry's media folder.
function saveArt(id, slot, buf, source, now = Date.now()) {
  const spec = checkSlot(slot);
  const img = decode(buf);
  const { width, height } = img.getSize();
  const size = fitSize(width, height, spec.w, spec.h);
  const out = size.width === width && size.height === height ? img : img.resize({ ...size, quality: 'best' });
  writeReplacing(artPath(id, slot), spec.ext === 'png' ? out.toPNG() : out.toJPEG(JPEG_QUALITY));
  return { path: fileNameFor(slot), source, updatedAt: now };
}

function readImageFile(file) {
  if (typeof file !== 'string' || !path.isAbsolute(file)) throw new Error('Choose an image file');
  let stat;
  try {
    stat = fs.statSync(file);
  } catch {
    throw new Error("Can't find that file");
  }
  if (!stat.isFile()) throw new Error('Choose an image file');
  if (stat.size > MAX_FILE_BYTES) throw new Error('That image is over 25 MB');
  return fs.readFileSync(file);
}

// Downloads an image over http(s), at most MAX_DOWNLOAD_BYTES of it.
async function downloadImage(url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error('That is not a web address');
  }
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') throw new Error('Use an http or https address');
  let res;
  try {
    res = await electron.net.fetch(parsed.href, { signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS) });
  } catch {
    throw new Error("Couldn't download that image");
  }
  if (!res.ok) throw new Error(`The site answered ${res.status}`);
  const length = Number(res.headers.get('content-length'));
  if (length > MAX_DOWNLOAD_BYTES) throw new Error('That image is over 15 MB');
  const buf = await require('../icons').readCapped(res, MAX_DOWNLOAD_BYTES + 1);
  if (buf.length > MAX_DOWNLOAD_BYTES) throw new Error('That image is over 15 MB');
  return buf;
}

function clearArt(id, slot) {
  fs.rmSync(artPath(id, slot), { force: true });
}

// The art a library entry may show: slots whose file is really there.
function existingArt(entry) {
  const out = {};
  for (const slot of IMAGE_SLOTS) {
    const m = entry.media && entry.media[slot];
    if (m && library.isValidId(entry.id) && fs.existsSync(artPath(entry.id, slot))) out[slot] = m;
  }
  return out;
}

// lpmedia://art/<uuid>/<slot>[?v=…] -> the file to serve, or null for any
// other shape. Only those two parts are read, both checked against fixed
// patterns, so no other file can be reached through the protocol.
function resolveArtUrl(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return null;
  }
  if (u.protocol !== 'lpmedia:' || u.hostname !== 'art') return null;
  const parts = u.pathname.split('/').filter(Boolean);
  if (parts.length !== 2) return null;
  const [id, slot] = parts;
  if (!library.isValidId(id) || !IMAGE_SLOTS.includes(slot)) return null;
  return artPath(id, slot);
}

module.exports = {
  saveArt,
  readImageFile,
  downloadImage,
  clearArt,
  existingArt,
  resolveArtUrl,
  artPath,
  fitSize,
  fileNameFor,
  SLOTS,
  IMAGE_SLOTS,
};
