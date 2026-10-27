// "Reinstall this version" (Settings → Repair): downloads the installer of
// the release matching the running version - the v<version> tag, not the
// latest release - checks it against that release's latest.yml, and hands
// back its path for main to run. Works like an update, just pinned to the
// version already installed. `fetchImpl` is Electron's net.fetch in the app
// and a stub in tests.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const REPO = 'Jurakii/Launchpad';

function releaseBase(version) {
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(String(version))) throw new Error(`"${version}" isn't a release version.`);
  return `https://github.com/${REPO}/releases/download/v${version}/`;
}

// The two fields electron-builder writes at the top level of latest.yml.
// Not a YAML parser: the file is machine-written and only these are needed.
function parseLatestYml(text) {
  const field = (name) => {
    const m = String(text).match(new RegExp(`^${name}:\\s*['"]?([^'"\\r\\n]+?)['"]?\\s*$`, 'm'));
    return m ? m[1] : null;
  };
  const file = field('path');
  const sha512 = field('sha512');
  if (!file || !sha512) throw new Error("The release's latest.yml has no installer listed.");
  // The name is joined onto the release URL and a temp folder: no path parts.
  if (file !== path.basename(file) || !/\.exe$/i.test(file)) throw new Error("The release's installer name isn't usable.");
  return { file, sha512, version: field('version') };
}

async function fetchOk(fetchImpl, url) {
  const res = await fetchImpl(url);
  if (!res.ok) {
    throw new Error(
      res.status === 404
        ? "This version's release isn't on GitHub (a dev build, or the release was removed)."
        : `Download failed (${res.status}).`
    );
  }
  return res;
}

// Resolves to the downloaded, verified installer's path in `tempDir`.
async function download({ version, fetchImpl, tempDir }) {
  const base = releaseBase(version);
  const info = parseLatestYml(await (await fetchOk(fetchImpl, `${base}latest.yml`)).text());
  if (info.version && info.version !== String(version)) throw new Error(`That release is for version ${info.version}.`);
  const buf = Buffer.from(await (await fetchOk(fetchImpl, base + encodeURIComponent(info.file))).arrayBuffer());
  const actual = crypto.createHash('sha512').update(buf).digest('base64');
  if (actual !== info.sha512) throw new Error("The downloaded installer doesn't match the release's checksum. Try again.");
  fs.mkdirSync(tempDir, { recursive: true });
  const dest = path.join(tempDir, info.file);
  fs.writeFileSync(dest, buf);
  return dest;
}

module.exports = { releaseBase, parseLatestYml, download, REPO };
