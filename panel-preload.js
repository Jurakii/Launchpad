const { contextBridge, ipcRenderer } = require('electron');

// The organizer panel (renderer/panel.html): main sends it what to show, and
// it reports clicks on and hovers over the zones of its layouts, clicks on
// its grid button, and, on the panel carrying Launchpad's top bar, clicks on
// and drops onto the toolbar's breadcrumb and buttons.
contextBridge.exposeInMainWorld('panelAPI', {
  onState: (callback) => ipcRenderer.on('panel:state', (_e, state) => callback(state)),
  snap: (layoutId, zoneId) => ipcRenderer.invoke('panel:snap', { layoutId, zoneId }),
  hover: (zone) => ipcRenderer.invoke('panel:hover', zone), // { layoutId, zoneId } or null
  openGrid: () => ipcRenderer.invoke('panel:openGrid'),
  // action: 'crumb' | 'crumbMenu' | 'drop' | 'layouts' | 'settings' | 'newIsland' | 'addApp' | 'fullscreen';
  // detail: { id } (an island, or null for Home), plus { ids } for a drop and
  // { x, y } (screen DIP) for a crumb's menu.
  launchpad: (action, detail) => ipcRenderer.invoke('panel:launchpad', { action, ...detail }),
});
