import * as p from "@clack/prompts";
import { platform, hostname } from "os";
import { writeFileSync, mkdirSync, existsSync } from "fs";
import { join } from "path";
import { $ } from "bun";
import { discoverServers, type ServerInfo } from "./discover";
import { scanMonitors, KNOWN_INPUTS, monitorLabel, monitorHint, type MonitorInfo } from "./monitor-scan";
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
    hint: inp.vcp + (currentInput === inp.value ? " (current)" : ""),
  }));
}

// identify a monitor by briefly blanking its screen
async function identifyMonitor(monitorId: string): Promise<boolean> {
  try {
    await $`softkvm identify ${monitorId}`.quiet();
    return true;
  } catch {
    return false;
  }
}

// assign a unique display label to each monitor, appending an index when
// names collide so the multiselect can distinguish them
function assignUniqueLabels(monitors: MonitorInfo[]): Map<string, string> {
  const labels = new Map<string, string>();
  const nameCounts = new Map<string, number>();

  // count how many times each base label appears
  for (const mon of monitors) {
    const base = monitorLabel(mon);
    nameCounts.set(base, (nameCounts.get(base) ?? 0) + 1);
  }

  // assign labels, appending index for duplicates
  const nameIndex = new Map<string, number>();
  for (const mon of monitors) {
    const base = monitorLabel(mon);
    if (nameCounts.get(base)! > 1) {
      const idx = (nameIndex.get(base) ?? 0) + 1;
      nameIndex.set(base, idx);
      labels.set(mon.id, `${base} (#${idx})`);
    } else {
      labels.set(mon.id, base);
    }
  }
  return labels;
}

async function main() {
  p.intro("softkvm setup");

  // discover servers and agents on the network
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

  // remote machine configuration
  let serverAddress: string | undefined;
  let serverName: string | undefined;

  if (role === "agent") {
    // agent needs to know the server
    if (hasServer) {
      let selectedServer: ServerInfo;
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
      serverAddress = selectedServer.ip;
      serverName = selectedServer.name;
    } else {
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
    }
  } else {
    // server role: client name is optional, agents identify themselves on connect
    // TODO: discover agents on the network and let user pick
    const wantClient = await p.confirm({
      message: "do you want to configure a client machine now? (can be added later when it connects)",
      initialValue: false,
    });

    if (p.isCancel(wantClient)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }

    if (wantClient) {
      const name = await p.text({
        message: "client machine name?",
        placeholder: "MacBook",
        validate: (v) => (v.length === 0 ? "name cannot be empty" : undefined),
      });
      if (p.isCancel(name)) {
        p.cancel("setup cancelled");
        process.exit(0);
      }
      serverName = name;
    }
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
    const labels = assignUniqueLabels(monitors);

    // offer to identify monitors if there are multiple
    if (monitors.length > 1) {
      const wantIdentify = await p.confirm({
        message: "want to identify monitors? (each screen will blank briefly so you can tell which is which)",
        initialValue: true,
      });

      if (!p.isCancel(wantIdentify) && wantIdentify) {
        for (const mon of monitors) {
          const label = labels.get(mon.id) ?? mon.id;
          const hint = monitorHint(mon);
          const go = await p.confirm({
            message: `blank "${label}"${hint ? ` (${hint})` : ""}? watch your screens`,
            initialValue: true,
          });
          if (p.isCancel(go)) break;
          if (go) {
            spinner.start(`blanking "${label}" for 3 seconds...`);
            await identifyMonitor(mon.id);
            spinner.stop(`"${label}" identification done`);
          }
        }
      }
    }

    // let user choose which monitors to use
    const selected = await p.multiselect({
      message: "which monitors should softkvm control?",
      options: monitors.map((mon) => ({
        value: mon.id,
        label: labels.get(mon.id) ?? mon.id,
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
      const label = labels.get(mon.id) ?? mon.id;
      p.log.info(`configuring ${label}`);

      const localInput = await p.select({
        message: `input on "${label}" connected to this machine (${machineName})?`,
        options: inputOptions(mon.current_input),
      });
      if (p.isCancel(localInput)) {
        p.cancel("setup cancelled");
        process.exit(0);
      }

      // only ask for remote input if we have a remote machine configured
      let remoteInput: string | undefined;
      if (serverName) {
        const ri = await p.select({
          message: `input on "${label}" connected to "${serverName}"?`,
          options: inputOptions(null),
        });
        if (p.isCancel(ri)) {
          p.cancel("setup cancelled");
          process.exit(0);
        }
        remoteInput = ri as string;
      }

      monitorSetups.push({
        name: label,
        monitorId: mon.id,
        localInput: localInput as string,
        remoteInput: remoteInput ?? "",
        remoteMachineName: serverName ?? "",
      });
    }
  }

  // screen layout (only if we have a remote machine)
  let direction: string | undefined;
  if (serverName) {
    const dir = await p.select({
      message: `where is "${serverName}" relative to this machine?`,
      options: [
        { value: "left", label: "Left" },
        { value: "right", label: "Right" },
        { value: "up", label: "Above" },
        { value: "down", label: "Below" },
      ],
    });

    if (p.isCancel(dir)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }
    direction = dir as string;
  }

  // generate config
  const answers: SetupAnswers = {
    role: role as "orchestrator" | "agent",
    machineName,
    os: detectOs(),
    serverName,
    serverAddress,
    monitors: monitorSetups,
    layout: direction && serverName
      ? { direction: direction as "left" | "right" | "up" | "down", neighborName: serverName }
      : undefined,
  };

  const config = generateConfig(answers);
  const dir2 = configDir();
  const configPath = join(dir2, "softkvm.toml");

  if (!existsSync(dir2)) {
    mkdirSync(dir2, { recursive: true });
  }
  writeFileSync(configPath, config);

  p.log.success(`configuration written to ${configPath}`);

  // show next steps
  const steps = [];
  if (role === "orchestrator") {
    steps.push("start the server: softkvm-orchestrator --config " + configPath);
    if (!serverName) {
      steps.push("when your other machine is ready, run softkvm-agent on it");
      steps.push("the agent will connect and identify itself automatically");
    }
  } else {
    steps.push("start the agent: softkvm-agent --config " + configPath);
  }
  steps.push("");
  steps.push("run `softkvm status` to check health");
  steps.push("run `softkvm scan` to list detected monitors");

  p.note(steps.join("\n"), "next steps");
  p.outro("setup complete");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
