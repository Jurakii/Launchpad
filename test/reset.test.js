const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { freshUserData, freshRequire, listDir } = require('./helpers');

function setup() {
  const dir = freshUserData();
  const w = (name, data) => fs.writeFileSync(path.join(dir, name), JSON.stringify(data));
  fs.mkdirSync(path.join(dir, 'icons'));
  fs.writeFileSync(path.join(dir, 'icons', 'a.png'), 'png-bytes');
  fs.mkdirSync(path.join(dir, 'files', 'thumbs'), { recursive: true });
  fs.mkdirSync(path.join(dir, 'media', 'x'), { recursive: true });
  w('apps.json', [{ id: 'a', kind: 'app', name: 'A', iconPath: path.join(dir, 'icons', 'a.png') }]);
  w('apps.json.bak', []);
  w('layouts.json', { version: 1, gap: 4, layouts: [] });
  w('settings.json', { hotkey: 'Alt+Space', controller: { deadZone: 0.5 }, libraries: [{ root: 'D:\\Games' }] });
  w('library.json', [{ id: 'm' }]);
  w('profiles.json', { profiles: [] });
  return { dir, reset: freshRequire('../lib/reset') };
}

test('reset keeps a pre-reset copy, clears Launchpad back to a first run and leaves Media alone', () => {
  const { dir, reset } = setup();
  const res = reset.reset({ now: new Date('2026-01-02T03:04:05Z') });
  assert.ok(res.ok);
  const files = listDir(dir);
  for (const gone of ['apps.json', 'apps.json.bak', 'layouts.json', 'icons', 'files']) assert.ok(!files.includes(gone), gone);
  for (const kept of ['library.json', 'profiles.json', 'media']) assert.ok(files.includes(kept), kept);
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(path.join(dir, 'settings.json'), 'utf-8')), {
    controller: { deadZone: 0.5 },
    libraries: [{ root: 'D:\\Games' }],
  });
  assert.strictEqual(path.basename(res.preReset), 'pre-reset-2026-01-02T03-04-05-000Z.json');
  const pre = JSON.parse(fs.readFileSync(res.preReset, 'utf-8'));
  assert.strictEqual(pre.apps[0].name, 'A');
  assert.strictEqual(Buffer.from(pre.apps[0].iconData, 'base64').toString(), 'png-bytes');
  assert.strictEqual(pre.layouts.gap, 4);
  assert.strictEqual(pre.settings.hotkey, 'Alt+Space');
});

test('the pre-reset copy is written before anything is deleted, and a failure part-way leaves it in place', () => {
  const { dir, reset } = setup();
  const realRm = fs.rmSync;
  let preResetSeenFirst = null;
  fs.rmSync = (p, opts) => {
    if (preResetSeenFirst === null) preResetSeenFirst = listDir(dir).some((f) => f.startsWith('pre-reset-'));
    if (path.basename(p) === 'layouts.json') throw Object.assign(new Error('busy'), { code: 'EBUSY' });
    return realRm(p, opts);
  };
  try {
    assert.throws(() => reset.reset(), /busy/);
  } finally {
    fs.rmSync = realRm;
  }
  assert.strictEqual(preResetSeenFirst, true);
  assert.ok(listDir(dir).some((f) => f.startsWith('pre-reset-')));
});
