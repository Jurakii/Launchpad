const { contextBridge, ipcRenderer } = require('electron');

// The grid placer (renderer/grid.html): main sends it what to show (null to
// clear it), and it sends back the cells picked, or that it was cancelled.
contextBridge.exposeInMainWorld('gridAPI', {
  onState: (callback) => ipcRenderer.on('grid:state', (_e, state) => callback(state)),
  place: (block) => ipcRenderer.invoke('grid:place', block), // { x, y, w, h } in cells
  cancel: () => ipcRenderer.invoke('grid:cancel'),
});
