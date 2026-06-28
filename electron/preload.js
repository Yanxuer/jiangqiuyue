const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
    showMainWindow: () => ipcRenderer.invoke('window-control', 'show-main'),
    hideFloatWindow: () => ipcRenderer.invoke('window-control', 'hide-float'),
    showFloatWindow: () => ipcRenderer.invoke('window-control', 'show-float'),
    getResourcePath: (relativePath) => ipcRenderer.invoke('get-resource-path', relativePath),
    getLogPath: () => ipcRenderer.invoke('get-log-path'),
    startDrag: () => ipcRenderer.send('window-drag-start'),
    sendToMain: (channel, data) => ipcRenderer.send(channel, data),
    onFromMain: (channel, callback) => ipcRenderer.on(channel, (event, ...args) => callback(...args)),
    // 日志系统
    log: (...args) => ipcRenderer.invoke('renderer-log', 'info', ...args),
    logError: (...args) => ipcRenderer.invoke('renderer-log', 'error', ...args),
    // 后端状态通知
    onBackendStatus: (callback) => {
        ipcRenderer.on('backend-status', (event, ready) => callback(ready));
    },
    // 文件夹选择
    selectDirectory: () => ipcRenderer.invoke('select-directory')
});
