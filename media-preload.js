const { contextBridge, ipcRenderer, webUtils } = require('electron');

// Media Mode (renderer/media/): the library, players and emulators, library
// folders, and launching. Every channel answers only this window.
contextBridge.exposeInMainWorld('mediaAPI', {
  library: () => ipcRenderer.invoke('media:library'),
  platforms: () => ipcRenderer.invoke('media:platforms'),
  profiles: () => ipcRenderer.invoke('media:profiles'),
  saveProfiles: (data) => ipcRenderer.invoke('media:saveProfiles', data),
  pickProgram: () => ipcRenderer.invoke('media:pickProgram'),
  pickFolder: () => ipcRenderer.invoke('media:pickFolder'),
  scanPreview: (request) => ipcRenderer.invoke('media:scanPreview', request),
  addLibrary: (request) => ipcRenderer.invoke('media:addLibrary', request),
  launch: (id) => ipcRenderer.invoke('media:launch', id),
  running: () => ipcRenderer.invoke('media:running'),
  stop: (id) => ipcRenderer.invoke('media:stop', id),
  onExited: (callback) => ipcRenderer.on('media:exited', (_e, info) => callback(info)),
  close: () => ipcRenderer.invoke('media:close'),
  setArtFile: (id, slot) => ipcRenderer.invoke('media:setArtFile', id, slot),
  // A dropped File -> its path on disk (the page can't see paths itself).
  setArtDropped: (id, slot, file) => ipcRenderer.invoke('media:setArtPath', id, slot, webUtils.getPathForFile(file)),
  setArtUrl: (id, slot, url) => ipcRenderer.invoke('media:setArtUrl', id, slot, url),
  clearArt: (id, slot) => ipcRenderer.invoke('media:clearArt', id, slot),
  controller: () => ipcRenderer.invoke('media:controller'),
  saveController: (data) => ipcRenderer.invoke('media:saveController', data),
});
