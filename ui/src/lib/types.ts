export type OsType = "windows" | "macos" | "linux";
export type MachineRole = "server" | "client";
export type Direction = "left" | "right" | "up" | "down";
export type DdcHealth = "healthy" | "degraded" | "unavailable";

export interface Machine {
  name: string;
  os: OsType;
  role: MachineRole;
  online: boolean;
  active: boolean;
}

export interface Monitor {
  name: string;
  id: string;
  currentInput: string;
  ddcHealth: DdcHealth;
  connectionType: string;
  inputs: Record<string, string>;
}

export interface LayoutEntry {
  machineName: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface KeyboardMapping {
  intent: string;
  mac: string;
  windows: string;
  enabled: boolean;
}

export interface BehaviorConfig {
  switchDelay: number;
  focusLockHotkey: string;
  quickSwitchHotkey: string;
  quickSwitchBackHotkey: string;
  adaptiveSwitchDelay: boolean;
  clipboardSharing: boolean;
  clipboardMaxSizeKb: number;
  toastNotifications: boolean;
  toastDurationMs: number;
  idleTimeoutMin: number;
}

export interface LogEntry {
  timestamp: string;
  level: "trace" | "debug" | "info" | "warn" | "error";
  message: string;
  source: string;
}

export interface DdcCommand {
  timestamp: string;
  monitorId: string;
  command: string;
  value: string;
  success: boolean;
  durationMs: number;
}

export interface AppState {
  machines: Machine[];
  monitors: Monitor[];
  layout: LayoutEntry[];
  keyboard: {
    autoRemap: boolean;
    translations: KeyboardMapping[];
  };
  behavior: BehaviorConfig;
  logs: LogEntry[];
  ddcHistory: DdcCommand[];
  deskflowStatus: "running" | "stopped" | "restarting";
  focusLocked: boolean;
}
