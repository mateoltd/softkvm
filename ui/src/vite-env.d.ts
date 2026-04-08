/// <reference types="vite/client" />

interface Window {
  api?: {
    getState: () => Promise<unknown>;
    getTheme: () => Promise<string>;
    switchMachine: (name: string) => void;
    showToast: (name: string, os: string) => void;
    onThemeChanged: (callback: (theme: string) => void) => void;
    onStateChanged: (callback: (state: unknown) => void) => void;
  };
}
