// preload.js — le seul pont entre l'interface et le système (même fichier dans
// les trois apps). L'interface n'a ni Node ni accès disque : elle appelle des
// canaux nommés, que main.js implémente un par un.
'use strict';

const { contextBridge, ipcRenderer, webUtils } = require('electron');

contextBridge.exposeInMainWorld('api', {
  call: (channel, ...args) => ipcRenderer.invoke('call', channel, ...args),
  on: (event, handler) => {
    const listener = (_e, ...args) => handler(...args);
    ipcRenderer.on(event, listener);
    return () => ipcRenderer.removeListener(event, listener);
  },
  pathForFile: (file) => webUtils.getPathForFile(file),
});
