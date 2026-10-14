const { contextBridge, ipcRenderer } = require('electron');

// The scenes menu (renderer/scenes-menu.html): main sends it the scenes to
// list, and it reports its height after drawing, and what was picked.
contextBridge.exposeInMainWorld('scenesMenuAPI', {
  onState: (callback) => ipcRenderer.on('scenesMenu:state', (_e, state) => callback(state)),
  resize: (height) => ipcRenderer.invoke('scenesMenu:resize', height),
  apply: (id) => ipcRenderer.invoke('scenesMenu:apply', id),
  save: () => ipcRenderer.invoke('scenesMenu:save'),
  manage: () => ipcRenderer.invoke('scenesMenu:manage'),
  close: () => ipcRenderer.invoke('scenesMenu:close'),
});
