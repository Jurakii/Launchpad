const fs = require('fs');
const path = require('path');
const { app, nativeImage, net } = require('electron');
const { iconsDir } = require('./store');
const favicon = require('./favicon');

async function extractIcon(targetPath) {
  try {
    const img = await app.getFileIcon(targetPath, { size: 'large' });
    if (img && !img.isEmpty()) return img;
  } catch {
    // fall through to placeholder
  }
  return null;
}

function saveIconForId(id, image) {
  const dest = path.join(iconsDir(), `${id}.png`);
  fs.writeFileSync(dest, image.toPNG());
  return dest;
}

async function extractAndSaveIcon(id, targetPath) {
  const img = await extractIcon(targetPath);
  if (!img) return null;
  return saveIconForId(id, img);
}

const FAVICON_SIZE = 128;
const SITE_ICON_BUDGET_MS = 10000; // whole site-direct attempt, then fall back
const REQUEST_TIMEOUT_MS = 5000;
const MAX_HTML_BYTES = 512 * 1024; // icon <link>s live in <head>
const MAX_ICON_BYTES = 1024 * 1024;
const MAX_LINK_CANDIDATES = 3;

// Reads at most `limit` bytes of a response body, then stops the download.
async function readCapped(res, limit) {
  const reader = res.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    while (total < limit) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
      total += value.length;
    }
  } finally {
    reader.cancel().catch(() => {});
  }
  return Buffer.concat(chunks).subarray(0, limit);
}

function imageFromBytes(buf) {
  let img = null;
  if (favicon.isPng(buf) || favicon.isJpeg(buf)) {
    img = nativeImage.createFromBuffer(buf);
  } else if (favicon.isIco(buf)) {
    // nativeImage can't read .ico from a buffer, and most /favicon.ico
    // files really are ICO, not a renamed PNG.
    const decoded = favicon.decodeIco(buf);
    if (decoded && decoded.png) img = nativeImage.createFromBuffer(decoded.png);
    else if (decoded) img = nativeImage.createFromBitmap(decoded.bitmap, { width: decoded.width, height: decoded.height });
  }
  if (!img || img.isEmpty()) return null;
  const { width } = img.getSize();
  return width > FAVICON_SIZE ? img.resize({ width: FAVICON_SIZE, height: FAVICON_SIZE, quality: 'best' }) : img;
}

async function fetchImage(url, signal) {
  try {
    const res = await net.fetch(url, { signal: AbortSignal.any([signal, AbortSignal.timeout(REQUEST_TIMEOUT_MS)]) });
    if (!res.ok) return null;
    return imageFromBytes(await readCapped(res, MAX_ICON_BYTES));
  } catch {
    return null;
  }
}

// The site's own icon: the best <link rel="icon"> / apple-touch-icon on
// the page, then /favicon.ico. Talks only to the site the user added.
async function fetchSiteIcon(pageUrl) {
  const signal = AbortSignal.timeout(SITE_ICON_BUDGET_MS);
  let finalUrl = pageUrl;
  let candidates = [];
  try {
    const res = await net.fetch(pageUrl.href, { signal: AbortSignal.any([signal, AbortSignal.timeout(REQUEST_TIMEOUT_MS)]) });
    if (res.url) finalUrl = new URL(res.url); // after redirects, e.g. example.com -> www.example.com
    if (res.ok && /html/i.test(res.headers.get('content-type') || '')) {
      const html = (await readCapped(res, MAX_HTML_BYTES)).toString('utf-8');
      candidates = favicon.findIconLinks(html, finalUrl.href).slice(0, MAX_LINK_CANDIDATES);
    } else {
      res.body?.cancel().catch(() => {});
    }
  } catch {
    // Page didn't load - /favicon.ico may still be there.
  }

  const fallbacks = [new URL('/favicon.ico', finalUrl).href, new URL('/favicon.ico', pageUrl).href];
  for (const url of new Set([...candidates, ...fallbacks])) {
    if (signal.aborted) break;
    const img = await fetchImage(url, signal);
    if (img) return img;
  }
  return null;
}

// Google's favicon service finds icons for sites that make them hard to
// get (no /favicon.ico, SVG only, bot-blocking), but it tells Google which
// sites are in the launcher - hence opt-out via settings.
async function fetchGoogleFavicon(hostname) {
  const url = `https://www.google.com/s2/favicons?sz=${FAVICON_SIZE}&domain=${encodeURIComponent(hostname)}`;
  return fetchImage(url, AbortSignal.timeout(REQUEST_TIMEOUT_MS + 1000));
}

async function fetchFavicon(url, { googleFallback = true } = {}) {
  const pageUrl = favicon.normalizeUrl(url);
  if (!pageUrl) return null;
  return (await fetchSiteIcon(pageUrl)) || (googleFallback ? await fetchGoogleFavicon(pageUrl.hostname) : null);
}

async function fetchAndSaveFavicon(id, url, options) {
  const img = await fetchFavicon(url, options);
  if (!img) return null;
  return saveIconForId(id, img);
}

function saveCustomIcon(id, sourcePath) {
  const img = nativeImage.createFromPath(sourcePath);
  if (img.isEmpty()) throw new Error('Could not read image file');
  const resized = img.resize({ width: 128, height: 128 });
  return saveIconForId(id, resized);
}

function iconDataUrl(iconPath) {
  if (!iconPath || !fs.existsSync(iconPath)) return null;
  const img = nativeImage.createFromPath(iconPath);
  return img.isEmpty() ? null : img.toDataURL();
}

// Icon extraction is a slow shell call, and the same exe gets asked about
// over and over (every poll of the taskbar's window list, every focus change
// for the organizer panel) - cache per path so it only happens once per
// unique running app.
const exeIconCache = new Map();

async function exeIconDataUrl(exePath) {
  if (!exePath) return null;
  if (exeIconCache.has(exePath)) return exeIconCache.get(exePath);
  let dataUrl = null;
  try {
    const img = await extractIcon(exePath);
    dataUrl = img ? img.toDataURL() : null;
  } catch {
    dataUrl = null;
  }
  exeIconCache.set(exePath, dataUrl);
  return dataUrl;
}

module.exports = {
  extractIcon,
  exeIconDataUrl,
  extractAndSaveIcon,
  fetchFavicon,
  fetchAndSaveFavicon,
  saveCustomIcon,
  iconDataUrl,
};
