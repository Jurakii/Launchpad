const { contextBridge, ipcRenderer } = require('electron');

// The organizer panel (renderer/panel.html): main sends it what to show, and
// it reports clicks on and hovers over the zones of its layouts.
contextBridge.exposeInMainWorld('panelAPI', {
  onState: (callback) => ipcRenderer.on('panel:state', (_e, state) => callback(state)),
  snap: (layoutId, zoneId) => ipcRenderer.invoke('panel:snap', { layoutId, zoneId }),
  hover: (zone) => ipcRenderer.invoke('panel:hover', zone), // { layoutId, zoneId } or null
});
