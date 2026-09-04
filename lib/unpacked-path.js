const path = require('path');

// Files handed to an external process (PowerShell, here) can't be read
// through Electron's asar virtualization - that only works for Node's own
// patched fs/require inside this process. A bundled file passed to
// child_process must resolve to its real on-disk location, which for
// anything marked asarUnpack in package.json means swapping the path
// through app.asar for its app.asar.unpacked twin. In dev (no asar in the
// path at all) this is a no-op.
function unpackedPath(...segments) {
  const joined = path.join(...segments);
  return joined.replace(`${path.sep}app.asar${path.sep}`, `${path.sep}app.asar.unpacked${path.sep}`);
}

module.exports = { unpackedPath };
