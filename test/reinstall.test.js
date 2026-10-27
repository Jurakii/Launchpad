const test = require('node:test');
const assert = require('node:assert');
const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const reinstall = require('../lib/reinstall');

const YML = (sha) =>
  [
    'version: 1.0.4',
    'files:',
    '  - url: Launchpad-Setup-1.0.4.exe',
    `    sha512: ${sha}`,
    '    size: 7',
    'path: Launchpad-Setup-1.0.4.exe',
    `sha512: ${sha}`,
    "releaseDate: '2026-01-01T00:00:00.000Z'",
    '',
  ].join('\n');

test('releaseBase pins the tag to the running version', () => {
  assert.strictEqual(reinstall.releaseBase('1.0.4'), 'https://github.com/Jurakii/Launchpad/releases/download/v1.0.4/');
  assert.throws(() => reinstall.releaseBase('../latest'));
});

test('parseLatestYml reads the installer and refuses names with path parts', () => {
  assert.deepStrictEqual(reinstall.parseLatestYml(YML('abc=')), { file: 'Launchpad-Setup-1.0.4.exe', sha512: 'abc=', version: '1.0.4' });
  assert.throws(() => reinstall.parseLatestYml('path: ..\\evil.exe\nsha512: x'));
  assert.throws(() => reinstall.parseLatestYml('version: 1'));
});

function fakeFetch(files) {
  const urls = [];
  const fetchImpl = async (url) => {
    urls.push(url);
    const body = files[url.split('/').pop()];
    if (body === undefined) return { ok: false, status: 404 };
    return { ok: true, status: 200, text: async () => String(body), arrayBuffer: async () => Buffer.from(body) };
  };
  return { urls, fetchImpl };
}

test('download fetches from v<version>, checks the checksum and saves the installer', async () => {
  const exe = 'MZfake!';
  const sha = crypto.createHash('sha512').update(exe).digest('base64');
  const { urls, fetchImpl } = fakeFetch({ 'latest.yml': YML(sha), 'Launchpad-Setup-1.0.4.exe': exe });
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'launchpad-reinstall-'));
  try {
    const dest = await reinstall.download({ version: '1.0.4', fetchImpl, tempDir });
    assert.strictEqual(fs.readFileSync(dest, 'utf-8'), exe);
    assert.ok(urls.every((u) => u.includes('/download/v1.0.4/')));
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

test('download refuses a mismatched checksum and reports a missing release', async () => {
  const { fetchImpl } = fakeFetch({ 'latest.yml': YML('nope'), 'Launchpad-Setup-1.0.4.exe': 'MZ' });
  await assert.rejects(reinstall.download({ version: '1.0.4', fetchImpl, tempDir: os.tmpdir() }), /checksum/);
  const missing = fakeFetch({});
  await assert.rejects(reinstall.download({ version: '1.0.4', fetchImpl: missing.fetchImpl, tempDir: os.tmpdir() }), /isn't on GitHub/);
});
