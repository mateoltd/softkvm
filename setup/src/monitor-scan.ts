import { $ } from "bun";

export interface MonitorInfo {
  id: string;
  name: string;
  manufacturer: string;
  model: string;
  serial: string;
  ddc_supported: boolean;
  current_input_vcp: string | null;
  current_input: string | null;
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

// well-known MCCS VCP 0x60 input sources
export const KNOWN_INPUTS = [
  { value: "VGA1", label: "VGA 1", vcp: "0x01" },
  { value: "VGA2", label: "VGA 2", vcp: "0x02" },
  { value: "DVI1", label: "DVI 1", vcp: "0x03" },
  { value: "DVI2", label: "DVI 2", vcp: "0x04" },
  { value: "DisplayPort1", label: "DisplayPort 1", vcp: "0x0f" },
  { value: "DisplayPort2", label: "DisplayPort 2", vcp: "0x10" },
  { value: "HDMI1", label: "HDMI 1", vcp: "0x11" },
  { value: "HDMI2", label: "HDMI 2", vcp: "0x12" },
];

// build a display label for a monitor, handling unknown/missing fields
export function monitorLabel(mon: MonitorInfo): string {
  const name = mon.name === "Unknown" ? null : mon.name;
  const model = mon.model === "UNK" ? null : mon.model;
  const mfg = mon.manufacturer === "Unknown" ? null : mon.manufacturer;

  if (name) return name;
  if (mfg && model) return `${mfg} ${model}`;
  if (model) return model;
  return mon.id;
}

// build a hint string showing current input for a monitor
export function monitorHint(mon: MonitorInfo): string {
  const parts: string[] = [];
  if (mon.current_input && mon.current_input !== mon.current_input_vcp) {
    parts.push(`current: ${mon.current_input}`);
  } else if (mon.current_input_vcp) {
    parts.push(`current: ${mon.current_input_vcp}`);
  }
  if (mon.id !== "UNK:UNK:UNK") {
    parts.push(mon.id);
  }
  return parts.join(" | ");
}
