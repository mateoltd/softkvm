import type { AppState } from "./types";

export const mockState: AppState = {
  machines: [
    { name: "Windows-PC", os: "windows", role: "server", online: true, active: true },
    { name: "MacBook", os: "macos", role: "client", online: true, active: false },
  ],
  monitors: [
    {
      name: "Dell U2720Q",
      id: "DEL:U2720Q:SN12345",
      currentInput: "DisplayPort1",
      ddcHealth: "healthy",
      connectionType: "DisplayPort",
      inputs: { "Windows-PC": "DisplayPort1", "MacBook": "HDMI1" },
    },
  ],
  layout: [
    { machineName: "Windows-PC", x: 0, y: 0, width: 280, height: 160 },
    { machineName: "MacBook", x: 296, y: 0, width: 280, height: 160 },
  ],
  keyboard: {
    autoRemap: true,
    translations: [
      { intent: "app_switcher", mac: "meta+tab", windows: "alt+tab", enabled: true },
      { intent: "quit_app", mac: "meta+q", windows: "alt+F4", enabled: true },
      { intent: "search", mac: "meta+space", windows: "super+s", enabled: true },
      { intent: "screenshot", mac: "meta+shift+4", windows: "super+shift+s", enabled: true },
      { intent: "lock_screen", mac: "meta+ctrl+q", windows: "super+l", enabled: true },
    ],
  },
  behavior: {
    switchDelay: 250,
    focusLockHotkey: "ScrollLock",
    quickSwitchHotkey: "ctrl+alt+right",
    quickSwitchBackHotkey: "ctrl+alt+left",
    adaptiveSwitchDelay: true,
    clipboardSharing: true,
    clipboardMaxSizeKb: 1024,
    toastNotifications: true,
    toastDurationMs: 500,
    idleTimeoutMin: 30,
  },
  logs: [
    { timestamp: "14:32:01.123", level: "info", message: "screen transition: Windows-PC \u2192 MacBook at 1920,540", source: "switch_engine" },
    { timestamp: "14:32:01.168", level: "info", message: "DDC switch DEL:U2720Q:SN12345 \u2192 HDMI1 (45ms)", source: "ddc" },
    { timestamp: "14:32:01.170", level: "debug", message: "key interceptor rules updated: 5 translations active", source: "key_interceptor" },
    { timestamp: "14:31:55.002", level: "info", message: "deskflow-core server started (pid 12847)", source: "deskflow" },
    { timestamp: "14:31:54.891", level: "info", message: "configuration loaded: 2 machines, 1 monitor", source: "orchestrator" },
  ],
  ddcHistory: [
    { timestamp: "14:32:01.123", monitorId: "DEL:U2720Q:SN12345", command: "set_input", value: "HDMI1 (0x11)", success: true, durationMs: 45 },
    { timestamp: "14:30:22.456", monitorId: "DEL:U2720Q:SN12345", command: "set_input", value: "DisplayPort1 (0x0f)", success: true, durationMs: 38 },
    { timestamp: "14:30:22.400", monitorId: "DEL:U2720Q:SN12345", command: "get_input", value: "HDMI1 (0x11)", success: true, durationMs: 12 },
  ],
  deskflowStatus: "running",
  focusLocked: false,
};
