const { contextBridge, ipcRenderer } = require('electron');

// The window picker (renderer/window-picker.html): main sends it the windows
// to list, and it reports its height after drawing, and which was picked.
contextBridge.exposeInMainWorld('pickerAPI', {
  onState: (callback) => ipcRenderer.on('picker:state', (_e, state) => callback(state)),
  resize: (height) => ipcRenderer.invoke('picker:resize', height),
  pick: (hwnd) => ipcRenderer.invoke('picker:pick', hwnd),
  close: () => ipcRenderer.invoke('picker:close'),
});
