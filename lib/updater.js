const https = require('https');

const REPO = 'Jurakii/Launchpad';

function fetchLatestRelease() {
  return new Promise((resolve, reject) => {
    const req = https.get(
      `https://api.github.com/repos/${REPO}/releases/latest`,
      { headers: { 'User-Agent': 'Launchpad-App' } },
      (res) => {
        if (res.statusCode !== 200) {
          res.resume();
          reject(new Error(`GitHub returned status ${res.statusCode}`));
          return;
        }
        let data = '';
        res.on('data', (chunk) => (data += chunk));
        res.on('end', () => {
          try {
            resolve(JSON.parse(data));
          } catch (err) {
            reject(err);
          }
        });
      }
    );
    req.on('error', reject);
    req.setTimeout(8000, () => req.destroy(new Error('Timed out contacting GitHub')));
  });
}

// Numeric, segment-by-segment comparison so "1.10.0" correctly beats "1.2.0"
// (a plain string compare would get that backwards).
function isNewer(latest, current) {
  const a = latest.split('.').map(Number);
  const b = current.split('.').map(Number);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] || 0;
    const y = b[i] || 0;
    if (x !== y) return x > y;
  }
  return false;
}

async function checkForUpdate(currentVersion) {
  const release = await fetchLatestRelease();
  const latestVersion = (release.tag_name || '').replace(/^v/i, '');
  return {
    currentVersion,
    latestVersion,
    hasUpdate: !!latestVersion && isNewer(latestVersion, currentVersion),
    url: release.html_url || `https://github.com/${REPO}/releases`,
  };
}

module.exports = { checkForUpdate };
