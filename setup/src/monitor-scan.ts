import { $ } from "bun";

export interface MonitorInfo {
  id: string;
  name: string;
  manufacturer: string;
  model: string;
  currentInput: string | null;
  ddcSupported: boolean;
}

// scan for monitors by shelling out to softkvm scan --json
export async function scanMonitors(): Promise<MonitorInfo[]> {
  try {
    const result = await $`softkvm scan --json`.text();
    return JSON.parse(result);
  } catch {
    // softkvm binary not available yet or no monitors found
    return [];
  }
}
