import * as p from "@clack/prompts";
import { platform, hostname } from "os";
import { writeFileSync, mkdirSync, existsSync } from "fs";
import { join } from "path";
import { discoverServers, type ServerInfo } from "./discover";
import { scanMonitors, KNOWN_INPUTS, monitorLabel, monitorHint } from "./monitor-scan";
import { generateConfig, type SetupAnswers, type MonitorSetup } from "./config-gen";

function detectOs(): "windows" | "macos" | "linux" {
  const p = platform();
  if (p === "win32") return "windows";
  if (p === "darwin") return "macos";
  return "linux";
}

function configDir(): string {
  const os = detectOs();
  if (os === "macos") {
    return join(process.env.HOME ?? "~", "Library", "Application Support", "softkvm");
  }
  if (os === "windows") {
    return join(process.env.LOCALAPPDATA ?? join(process.env.HOME ?? "~", "AppData", "Local"), "softkvm");
  }
  return join(process.env.XDG_CONFIG_HOME ?? join(process.env.HOME ?? "~", ".config"), "softkvm");
}

// build input source options for a monitor, highlighting its current input
function inputOptions(currentInput: string | null) {
  return KNOWN_INPUTS.map((inp) => ({
    value: inp.value,
    label: inp.label,
    hint: inp.vcp + (currentInput === inp.label.replace(" ", "") || currentInput === inp.value
      ? " (current)"
      : ""),
  }));
}

async function main() {
  p.intro("softkvm setup");

  // discover servers
  const spinner = p.spinner();
  spinner.start("scanning network for existing softkvm servers");
  const servers = await discoverServers();
  spinner.stop(
    servers.length > 0
      ? `found ${servers.length} server(s): ${servers.map((s) => `${s.name} (${s.ip})`).join(", ")}`
      : "no servers found on the network"
  );

  // choose role
  const hasServer = servers.length > 0;
  const role = await p.select({
    message: "what role should this machine play?",
    options: [
      {
        value: hasServer ? "agent" : "orchestrator",
        label: hasServer ? "Client (recommended)" : "Server (recommended)",
        hint: hasServer ? "server detected on your network" : "no server found, this will be the first",
      },
      {
        value: hasServer ? "orchestrator" : "agent",
        label: hasServer ? "Server" : "Client",
      },
    ],
  });

  if (p.isCancel(role)) {
    p.cancel("setup cancelled");
    process.exit(0);
  }

  // machine name
  const machineName = await p.text({
    message: "machine name?",
    placeholder: hostname(),
    defaultValue: hostname(),
    validate: (v) => (v.length === 0 ? "name cannot be empty" : undefined),
  });

  if (p.isCancel(machineName)) {
    p.cancel("setup cancelled");
    process.exit(0);
  }

  // if agent, pick which server to connect to
  let selectedServer: ServerInfo | undefined;
  let serverAddress: string | undefined;
  let serverName: string | undefined;

  if (role === "agent" && hasServer) {
    if (servers.length === 1) {
      selectedServer = servers[0];
    } else {
      const choice = await p.select({
        message: "which server should this machine connect to?",
        options: servers.map((s) => ({
          value: s,
          label: `${s.name} (${s.ip}:${s.port})`,
        })),
      });
      if (p.isCancel(choice)) {
        p.cancel("setup cancelled");
        process.exit(0);
      }
      selectedServer = choice as ServerInfo;
    }
    serverAddress = selectedServer!.ip;
    serverName = selectedServer!.name;
  } else if (role === "agent") {
    // no server found, manual entry
    const addr = await p.text({
      message: "server IP address?",
      placeholder: "192.168.1.100",
      validate: (v) => (v.length === 0 ? "address cannot be empty" : undefined),
    });
    if (p.isCancel(addr)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }
    serverAddress = addr;

    const name = await p.text({
      message: "server machine name?",
      placeholder: "Windows-PC",
      validate: (v) => (v.length === 0 ? "name cannot be empty" : undefined),
    });
    if (p.isCancel(name)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }
    serverName = name;
  } else {
    // server role, ask for remote machine name
    const name = await p.text({
      message: "name of the other machine that will connect?",
      placeholder: "MacBook",
      validate: (v) => (v.length === 0 ? "name cannot be empty" : undefined),
    });
    if (p.isCancel(name)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }
    serverName = name;
  }

  // scan monitors
  spinner.start("scanning monitors for DDC/CI support");
  const monitors = await scanMonitors();
  spinner.stop(
    monitors.length > 0
      ? `found ${monitors.length} monitor(s) with DDC/CI support`
      : "no DDC/CI monitors detected (will configure manually later)"
  );

  const monitorSetups: MonitorSetup[] = [];

  if (monitors.length > 0) {
    // let user choose which monitors to use
    const selected = await p.multiselect({
      message: "which monitors should softkvm control?",
      options: monitors.map((mon) => ({
        value: mon.id,
        label: monitorLabel(mon),
        hint: monitorHint(mon),
      })),
      required: false,
    });

    if (p.isCancel(selected)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }

    const selectedIds = selected as string[];
    const selectedMonitors = monitors.filter((m) => selectedIds.includes(m.id));

    for (const mon of selectedMonitors) {
      const label = monitorLabel(mon);
      p.log.info(`configuring ${label}`);

      const localInput = await p.select({
        message: `input on "${label}" connected to this machine (${machineName})?`,
        options: inputOptions(mon.current_input),
      });
      if (p.isCancel(localInput)) {
        p.cancel("setup cancelled");
        process.exit(0);
      }

      const remoteInput = await p.select({
        message: `input on "${label}" connected to "${serverName}"?`,
        options: inputOptions(null),
      });
      if (p.isCancel(remoteInput)) {
        p.cancel("setup cancelled");
        process.exit(0);
      }

      monitorSetups.push({
        name: label,
        monitorId: mon.id,
        localInput: localInput as string,
        remoteInput: remoteInput as string,
        remoteMachineName: serverName!,
      });
    }
  }

  // screen layout
  const direction = await p.select({
    message: `where is "${serverName}" relative to this machine?`,
    options: [
      { value: "left", label: "Left" },
      { value: "right", label: "Right" },
      { value: "up", label: "Above" },
      { value: "down", label: "Below" },
    ],
  });

  if (p.isCancel(direction)) {
    p.cancel("setup cancelled");
    process.exit(0);
  }

  // generate config
  const answers: SetupAnswers = {
    role: role as "orchestrator" | "agent",
    machineName,
    os: detectOs(),
    serverName,
    serverAddress,
    monitors: monitorSetups,
    layout: {
      direction: direction as "left" | "right" | "up" | "down",
      neighborName: serverName!,
    },
  };

  const config = generateConfig(answers);
  const dir = configDir();
  const configPath = join(dir, "softkvm.toml");

  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  writeFileSync(configPath, config);

  p.log.success(`configuration written to ${configPath}`);

  // show next steps
  p.note(
    [
      role === "orchestrator"
        ? "start the server: softkvm-orchestrator --config " + configPath
        : "start the agent: softkvm-agent --config " + configPath,
      "",
      "run `softkvm status` to check health",
      "run `softkvm scan` to list detected monitors",
    ].join("\n"),
    "next steps"
  );

  p.outro("setup complete");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
