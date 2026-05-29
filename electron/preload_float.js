const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
    showMainWindow: () => ipcRenderer.invoke('window-control', 'show-main'),
    hideFloatWindow: () => ipcRenderer.invoke('window-control', 'hide-float'),
    setIgnoreMouseEvents: (ignore) => ipcRenderer.invoke('window-control', ignore ? 'set-ignore-mouse' : 'set-mouse-normal'),
    getResourcePath: (path) => ipcRenderer.invoke('get-resource-path', path),
    startDrag: () => ipcRenderer.send('window-drag-start'),
    dragWindow: (dx, dy) => ipcRenderer.invoke('window-drag-move', dx, dy),
    togglePin: (pinned) => ipcRenderer.invoke('window-toggle-pin', pinned),
    sendToMain: (channel, data) => ipcRenderer.send(channel, data),
    onFromMain: (channel, callback) => ipcRenderer.on(channel, (event, ...args) => callback(...args))
});
