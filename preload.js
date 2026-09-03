const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('launcherAPI', {
  listApps: () => ipcRenderer.invoke('apps:list'),
  addApp: (formData) => ipcRenderer.invoke('apps:add', formData),
  updateApp: (id, formData) => ipcRenderer.invoke('apps:update', { id, formData }),
  removeApp: (id) => ipcRenderer.invoke('apps:remove', id),
  launchApp: (id) => ipcRenderer.invoke('apps:launch', id),
  pickTarget: (kind) => ipcRenderer.invoke('dialog:pickTarget', kind),
  pickIcon: () => ipcRenderer.invoke('dialog:pickIcon'),
  extractIconPreview: (targetPath) => ipcRenderer.invoke('icon:extractPreview', targetPath),
  addFolder: (name, parentId) => ipcRenderer.invoke('folder:add', { name, parentId }),
  renameItem: (id, name) => ipcRenderer.invoke('item:rename', { id, name }),
  moveItem: (id, parentId) => ipcRenderer.invoke('item:move', { id, parentId }),
  togglePin: (id) => ipcRenderer.invoke('item:togglePin', id),
});
