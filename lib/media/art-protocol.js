// The lpmedia: scheme serves library art to the Media Mode window:
// lpmedia://art/<uuid>/<slot>?v=<updatedAt>. art.resolveArtUrl() decides
// what a URL may reach; anything else is a 404. `v` changes whenever the
// art does, so a replaced cover shows at once without cache headaches.
const { protocol, net } = require('electron');
const { pathToFileURL } = require('url');
const fs = require('fs');
const art = require('./art');

const SCHEME = 'lpmedia';

// Must run before app 'ready'. 'standard' gives the URLs a host and path,
// 'secure' lets the CSP-restricted page load them like its own files.
function registerScheme() {
  protocol.registerSchemesAsPrivileged([{ scheme: SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
}

function handleScheme() {
  protocol.handle(SCHEME, (request) => {
    const file = art.resolveArtUrl(request.url);
    if (!file || !fs.existsSync(file)) return new Response('Not found', { status: 404 });
    return net.fetch(pathToFileURL(file).toString());
  });
}

module.exports = { registerScheme, handleScheme, SCHEME };
