import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("api", {
  getState: () => ipcRenderer.invoke("get-state"),
  getTheme: () => ipcRenderer.invoke("get-theme"),
  switchMachine: (name: string) => ipcRenderer.send("switch-machine", name),
  showToast: (name: string, os: string) => ipcRenderer.send("show-toast", name, os),
  onThemeChanged: (callback: (theme: string) => void) => {
    ipcRenderer.on("theme-changed", (_e, theme) => callback(theme));
  },
  onStateChanged: (callback: (state: unknown) => void) => {
    ipcRenderer.on("state-changed", (_e, state) => callback(state));
  },
});
