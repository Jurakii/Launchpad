const fs = require('fs');
const path = require('path');
const { app, nativeImage } = require('electron');
const { iconsDir } = require('./store');

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

module.exports = { extractIcon, extractAndSaveIcon, saveCustomIcon, iconDataUrl };
