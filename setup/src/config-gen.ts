export interface SetupAnswers {
  role: "orchestrator" | "agent";
  machineName: string;
  os: "windows" | "macos" | "linux";
  serverName?: string;
  serverAddress?: string;
  monitors: MonitorSetup[];
  layout?: LayoutSetup;
  deskflowPath?: string;
}

export interface MonitorSetup {
  name: string;
  monitorId: string;
  localInput: string;
}

export interface LayoutSetup {
  direction: "left" | "right" | "up" | "down";
  neighborName: string;
}

// generate a softkvm.toml configuration from the setup answers
export function generateConfig(answers: SetupAnswers): string {
  const lines: string[] = [];

  lines.push(`[general]`);
  lines.push(`role = "${answers.role}"`);
  lines.push(`log_level = "info"`);
  lines.push(``);

  lines.push(`[deskflow]`);
  lines.push(`managed = true`);
  if (answers.deskflowPath) {
    lines.push(`binary_path = "${answers.deskflowPath}"`);
  }
  lines.push(`switch_delay = 250`);
  lines.push(`clipboard_sharing = true`);
  lines.push(``);

  // local machine
  lines.push(`[[machine]]`);
  lines.push(`name = "${answers.machineName}"`);
  lines.push(`role = "${answers.role === "orchestrator" ? "server" : "client"}"`);
  lines.push(`os = "${answers.os}"`);
  lines.push(``);

  // remote machine (from server discovery or manual entry)
  if (answers.serverName) {
    const remoteRole = answers.role === "orchestrator" ? "client" : "server";
    const remoteOs = answers.os === "macos" ? "windows" : "macos";
    lines.push(`[[machine]]`);
    lines.push(`name = "${answers.serverName}"`);
    lines.push(`role = "${remoteRole}"`);
    lines.push(`os = "${remoteOs}"`);
    lines.push(``);
  }

  // monitors
  for (const mon of answers.monitors) {
    lines.push(`[[monitor]]`);
    lines.push(`name = "${mon.name}"`);
    lines.push(`monitor_id = "${mon.monitorId}"`);
    lines.push(`connected_to = "${answers.machineName}"`);
    lines.push(``);
    lines.push(`[monitor.inputs]`);
    lines.push(`"${answers.machineName}" = "${mon.localInput}"`);
    lines.push(``);
  }

  // layout (only if we have a remote machine and direction)
  if (answers.layout && answers.serverName) {
    const opposite: Record<string, string> = {
      left: "right",
      right: "left",
      up: "down",
      down: "up",
    };
    lines.push(`[layout]`);
    lines.push(
      `"${answers.machineName}" = { ${answers.layout.direction} = "${answers.layout.neighborName}" }`
    );
    lines.push(
      `"${answers.layout.neighborName}" = { ${opposite[answers.layout.direction]} = "${answers.machineName}" }`
    );
    lines.push(``);
  }

  // network (agent-only)
  if (answers.role === "agent" && answers.serverAddress) {
    lines.push(`[network]`);
    lines.push(`orchestrator_address = "${answers.serverAddress}"`);
    lines.push(``);
  }

  return lines.join("\n");
}
