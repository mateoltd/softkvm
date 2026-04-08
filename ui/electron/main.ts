import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, nativeTheme, screen } from "electron";
import { join } from "path";
import { createConnection } from "net";

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let toastWindow: BrowserWindow | null = null;

// IPC bridge state — in production this connects to the Rust daemon via JSON-RPC
// over a unix socket (macOS/Linux) or named pipe (Windows)
const daemonState = {
  connected: false,
  machines: [] as Array<{ name: string; os: string; role: string; online: boolean; active: boolean }>,
  monitors: [] as Array<{ name: string; id: string; currentInput: string; ddcHealthy: boolean }>,
};

function createMainWindow() {
  const { width, height } = screen.getPrimaryDisplay().workAreaSize;
  mainWindow = new BrowserWindow({
    width: Math.min(960, width),
    height: Math.min(680, height),
    minWidth: 760,
    minHeight: 520,
    show: false,
    frame: false,
    titleBarStyle: "hidden",
    titleBarOverlay: process.platform === "darwin" ? {
      color: nativeTheme.shouldUseDarkColors ? "#0a0a0a" : "#fafafa",
      symbolColor: nativeTheme.shouldUseDarkColors ? "#a1a1aa" : "#52525b",
      height: 40,
    } : undefined,
    trafficLightPosition: { x: 14, y: 14 },
    backgroundColor: nativeTheme.shouldUseDarkColors ? "#0a0a0a" : "#fafafa",
    webPreferences: {
      preload: join(__dirname, "../preload/preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  if (process.env.VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(process.env.VITE_DEV_SERVER_URL);
  } else {
    mainWindow.loadFile(join(__dirname, "../../dist/index.html"));
  }

  mainWindow.once("ready-to-show", () => {
    mainWindow?.show();
  });

  mainWindow.on("close", (e) => {
    // minimize to tray instead of quitting
    if (process.platform !== "linux") {
      e.preventDefault();
      mainWindow?.hide();
    }
  });

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

function createTray() {
  // 16x16 tray icon — a minimal KVM glyph
  const icon = nativeImage.createFromDataURL(
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAARklEQVQ4y2NgGAWjIUABZGBsbPw/ISHhPzYMEkNXA9IMUoMNg8SQ1YDEsWGQGLIakBg2DFOMTc2oFwwfLxg0gXhUC4YPAAAU8SUZL3BXRQAAAABJRU5ErkJggg=="
  );
  tray = new Tray(icon);
  tray.setToolTip("softkvm");
  updateTrayMenu();

  tray.on("click", () => {
    if (mainWindow?.isVisible()) {
      mainWindow.hide();
    } else {
      if (!mainWindow) createMainWindow();
      mainWindow?.show();
      mainWindow?.focus();
    }
  });
}

function updateTrayMenu() {
  if (!tray) return;

  const menuItems: Electron.MenuItemConstructorOptions[] = [];

  if (daemonState.machines.length > 0) {
    for (const m of daemonState.machines) {
      const statusIcon = m.active ? "\u25cf" : m.online ? "\u25cb" : "\u25cc";
      const statusLabel = m.active ? "active" : m.online ? "online" : "offline";
      menuItems.push({
        label: `${statusIcon}  ${m.name}  —  ${statusLabel}`,
        enabled: false,
      });
    }
    menuItems.push({ type: "separator" });

    // switch buttons for non-active machines
    for (const m of daemonState.machines.filter(x => !x.active && x.online)) {
      menuItems.push({
        label: `Switch to ${m.name}`,
        click: () => {
          // TODO: send switch command via IPC to Rust daemon
        },
      });
    }
    if (daemonState.machines.some(x => !x.active && x.online)) {
      menuItems.push({ type: "separator" });
    }
  } else {
    menuItems.push({ label: "No machines configured", enabled: false });
    menuItems.push({ type: "separator" });
  }

  menuItems.push({
    label: "Settings...",
    click: () => {
      if (!mainWindow) createMainWindow();
      mainWindow?.show();
      mainWindow?.focus();
    },
  });

  menuItems.push({ type: "separator" });
  menuItems.push({ label: "Quit", click: () => app.quit() });

  tray.setContextMenu(Menu.buildFromTemplate(menuItems));
}

function showToast(machineName: string, os: string) {
  if (toastWindow && !toastWindow.isDestroyed()) {
    toastWindow.destroy();
  }

  const display = screen.getPrimaryDisplay();
  const { width } = display.workAreaSize;

  toastWindow = new BrowserWindow({
    width: 280,
    height: 56,
    x: Math.round(width / 2 - 140),
    y: 48,
    frame: false,
    transparent: true,
    alwaysOnTop: true,
    skipTaskbar: true,
    focusable: false,
    resizable: false,
    webPreferences: {
      preload: join(__dirname, "../preload/preload.js"),
      contextIsolation: true,
    },
  });

  // inline HTML for the toast — keeps it self-contained
  const isDark = nativeTheme.shouldUseDarkColors;
  const bg = isDark ? "rgba(24,24,27,0.92)" : "rgba(250,250,250,0.92)";
  const fg = isDark ? "#e4e4e7" : "#18181b";
  const sub = isDark ? "#71717a" : "#a1a1aa";
  const osIcon = os === "macos" ? "\uf179" : os === "windows" ? "\uf17a" : "\uf17c";

  const html = `<!DOCTYPE html>
<html><head><style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif;
    background: transparent; -webkit-app-region: no-drag; user-select: none;
  }
  .toast {
    background: ${bg}; backdrop-filter: blur(20px); -webkit-backdrop-filter: blur(20px);
    border: 1px solid ${isDark ? "rgba(63,63,70,0.5)" : "rgba(228,228,231,0.8)"};
    border-radius: 12px; padding: 12px 20px; display: flex; align-items: center; gap: 12px;
    box-shadow: 0 8px 32px rgba(0,0,0,${isDark ? "0.4" : "0.12"});
    animation: fadeIn 0.15s ease-out, fadeOut 0.15s ease-in 0.4s forwards;
  }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: #22c55e; flex-shrink: 0; }
  .name { font-size: 13px; font-weight: 600; color: ${fg}; letter-spacing: -0.01em; }
  .os { font-size: 11px; color: ${sub}; margin-left: auto; }
  @keyframes fadeIn { from { opacity: 0; transform: translateY(-8px); } to { opacity: 1; transform: translateY(0); } }
  @keyframes fadeOut { from { opacity: 1; } to { opacity: 0; transform: translateY(-4px); } }
</style></head><body>
  <div class="toast">
    <div class="dot"></div>
    <span class="name">${machineName}</span>
    <span class="os">${os}</span>
  </div>
</body></html>`;

  toastWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`);

  // auto-destroy after animation completes
  setTimeout(() => {
    if (toastWindow && !toastWindow.isDestroyed()) {
      toastWindow.destroy();
      toastWindow = null;
    }
  }, 600);
}

// IPC handlers
ipcMain.handle("get-state", () => daemonState);
ipcMain.handle("get-theme", () => nativeTheme.shouldUseDarkColors ? "dark" : "light");
ipcMain.on("show-toast", (_e, name: string, os: string) => showToast(name, os));
ipcMain.on("switch-machine", (_e, name: string) => {
  // TODO: forward to Rust daemon via JSON-RPC
  console.log(`switch requested: ${name}`);
});

nativeTheme.on("updated", () => {
  mainWindow?.webContents.send("theme-changed", nativeTheme.shouldUseDarkColors ? "dark" : "light");
});

// app lifecycle
app.whenReady().then(() => {
  createTray();
  createMainWindow();

  app.on("activate", () => {
    if (!mainWindow) createMainWindow();
    mainWindow?.show();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    // keep running in tray
  }
});
