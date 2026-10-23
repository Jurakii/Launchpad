// Media's part of a Launchpad backup (version 3): the library entries
// and the player and emulator profiles. Art is left out - it can be large
// and can be set or scraped again - and so are scraper keys.
const library = require('./library');
const players = require('./players');

function exportData() {
  return { library: library.load(), profiles: players.load() };
}

// Replaces the library and profiles with a backup's. A backup without them
// (version 1 or 2) leaves both alone. Returns how many entries came in, or
// null when there was nothing to import.
function importData(bundle) {
  if (!bundle || typeof bundle !== 'object' || Array.isArray(bundle)) return null;
  let count = null;
  if (Array.isArray(bundle.library)) count = library.replaceAll(bundle.library).length;
  if (bundle.profiles && typeof bundle.profiles === 'object') players.save(bundle.profiles);
  return count;
}

module.exports = { exportData, importData };
