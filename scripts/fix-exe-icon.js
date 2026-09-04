// electron-builder's Windows icon embedding goes through rcedit, which it
// downloads bundled inside the winCodeSign archive. On a machine without
// Windows Developer Mode enabled, extracting that archive fails (it contains
// symlinks, which requires a privilege normal accounts don't have), so
// win.icon in package.json's build config silently never gets applied and
// the packaged exe keeps Electron's default icon. This re-embeds our icon
// directly using resedit (a pure-JS PE resource editor, already a
// dependency of electron-builder) so `npm run dist` produces a correctly
// iconed exe regardless of that limitation.
const fs = require('fs');
const path = require('path');
const ResEdit = require('resedit');

const exePath = path.join(__dirname, '..', 'dist', 'win-unpacked', 'Launchpad.exe');
const icoPath = path.join(__dirname, '..', 'build', 'icon.ico');

if (!fs.existsSync(exePath)) {
  console.log(`fix-exe-icon: ${exePath} not found, skipping.`);
  process.exit(0);
}

const executable = ResEdit.NtExecutable.from(fs.readFileSync(exePath));
const resource = ResEdit.NtExecutableResource.from(executable);
const existingGroups = ResEdit.Resource.IconGroupEntry.fromEntries(resource.entries);
const iconFile = ResEdit.Data.IconFile.from(fs.readFileSync(icoPath));

for (const group of existingGroups) {
  ResEdit.Resource.IconGroupEntry.replaceIconsForResource(
    resource.entries,
    group.id,
    group.lang,
    iconFile.icons.map((item) => item.data)
  );
}

resource.outputResource(executable);
fs.writeFileSync(exePath, Buffer.from(executable.generate()));
console.log(`fix-exe-icon: embedded ${icoPath} into ${exePath}`);
