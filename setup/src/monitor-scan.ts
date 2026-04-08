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

// find the known input matching a VCP hex string, or null
export function knownInputByVcp(vcp: string | null): typeof KNOWN_INPUTS[number] | null {
  if (!vcp) return null;
  return KNOWN_INPUTS.find((inp) => inp.vcp === vcp) ?? null;
}

// resolve the detected input to the config value to store
// returns the well-known name (e.g. "DisplayPort1") or raw hex (e.g. "0x07")
export function detectedInputConfigValue(mon: MonitorInfo): string | null {
  if (!mon.current_input_vcp) return null;
  // current_input from scan is already the right format:
  // known inputs are "DisplayPort1", "HDMI1", etc.
  // non-standard are "0x07", "0x1b", etc.
  return mon.current_input ?? mon.current_input_vcp;
}

// human-readable label for the detected input (e.g. "DisplayPort 1 (0x0f)")
export function detectedInputLabel(mon: MonitorInfo): string | null {
  if (!mon.current_input_vcp) return null;
  const known = knownInputByVcp(mon.current_input_vcp);
  if (known) return `${known.label} (${known.vcp})`;
  return `${mon.current_input_vcp} (non-standard)`;
}

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
