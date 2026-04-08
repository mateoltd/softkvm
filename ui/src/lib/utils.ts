import { clsx, type ClassValue } from "clsx";

export function cn(...inputs: ClassValue[]) {
  return clsx(inputs);
}

export function osIcon(os: string): string {
  switch (os) {
    case "macos": return "\uF179";
    case "windows": return "\uF17A";
    case "linux": return "\uF17C";
    default: return "\uF108";
  }
}

export function osLabel(os: string): string {
  switch (os) {
    case "macos": return "macOS";
    case "windows": return "Windows";
    case "linux": return "Linux";
    default: return os;
  }
}

export function formatKey(key: string): string {
  const map: Record<string, string> = {
    meta: "\u2318",
    cmd: "\u2318",
    command: "\u2318",
    ctrl: "\u2303",
    control: "\u2303",
    alt: "\u2325",
    option: "\u2325",
    shift: "\u21E7",
    super: "\u229E",
    win: "\u229E",
    tab: "\u21E5",
    space: "\u2423",
    scrolllock: "ScrLk",
  };
  return map[key.toLowerCase()] ?? key;
}

export function renderShortcut(shortcut: string): string[] {
  return shortcut.split("+").map(formatKey);
}
