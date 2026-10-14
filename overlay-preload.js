const { contextBridge, ipcRenderer } = require('electron');

// The drag overlay (renderer/overlay.html) only listens: main sends it what
// to draw while a window is being dragged, and null to clear it.
contextBridge.exposeInMainWorld('overlayAPI', {
  onState: (callback) => ipcRenderer.on('overlay:state', (_e, state) => callback(state)),
});
