// Imports a backup with Launchpad 1.2.2's import logic (js-env.js) into a
// data folder, and prints { storedMissing, layoutsError, mediaError } as JSON.
// Run by crates/core/tests/compat.rs when node is on PATH:
//
//   node js-import.js <backup.json> <data folder>
const { useUserData, importBackup } = require('./js-env');

const [backup, dir] = process.argv.slice(2);
const m = useUserData(dir);
m.store.load(); // as the app does at startup: the import expects icons\ to exist
importBackup(m, backup)
  .then(({ storedMissing, layoutsError, mediaError }) => {
    process.stdout.write(JSON.stringify({ storedMissing, layoutsError, mediaError }));
  })
  .catch((err) => {
    console.error(err);
    process.exit(1);
  });
