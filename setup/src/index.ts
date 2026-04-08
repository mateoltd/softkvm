import * as p from "@clack/prompts";
import { platform, hostname } from "os";
import { writeFileSync, mkdirSync, existsSync } from "fs";
import { join } from "path";
import { $ } from "bun";
import { discoverServers, type ServerInfo } from "./discover";
import { scanMonitors, KNOWN_INPUTS, detectedInputConfigValue, detectedInputLabel, monitorLabel, monitorHint, type MonitorInfo } from "./monitor-scan";
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

  // scan monitors, filter to only those with working DDC/CI
  spinner.start("scanning monitors for DDC/CI support");
  const allMonitors = await scanMonitors();
  const monitors = allMonitors.filter((m) => m.ddc_supported);
  const skipped = allMonitors.length - monitors.length;
  let scanMsg = monitors.length > 0
    ? `found ${monitors.length} monitor(s) with DDC/CI support`
    : "no DDC/CI monitors detected (will configure manually later)";
  if (skipped > 0) {
    scanMsg += ` (${skipped} without DDC/CI skipped)`;
  }
  spinner.stop(scanMsg);

  const monitorSetups: MonitorSetup[] = [];

  if (monitors.length > 0) {
    const labels = assignUniqueLabels(monitors);

    // offer to identify monitors if there are multiple
    if (monitors.length > 1) {
      let identifying = true;
      while (identifying) {
        const identifyChoice = await p.select({
          message: "want to identify which screen is which? (blanks each monitor briefly)",
          options: [
            ...monitors.map((mon) => {
              const label = labels.get(mon.id) ?? mon.id;
              const hint = monitorHint(mon);
              return {
                value: mon.id,
                label: `blank "${label}"`,
                hint: hint || undefined,
              };
            }),
            { value: "__skip__", label: "skip identification" },
          ],
        });

        if (p.isCancel(identifyChoice) || identifyChoice === "__skip__") {
          identifying = false;
        } else {
          const mon = monitors.find((m) => m.id === identifyChoice)!;
          const label = labels.get(mon.id) ?? mon.id;
          p.log.warn(
            `blanking "${label}" for 3 seconds. if your current screen goes dark, that's this one.`
          );
          spinner.start(`blanking "${label}"...`);
          await identifyMonitor(mon.id);
          spinner.stop(`"${label}" restored`);
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

      // local input: use detected value when available
      let localInputValue = detectedInputConfigValue(mon);
      const localInputDisplay = detectedInputLabel(mon);

      if (localInputValue && localInputDisplay) {
        p.log.info(`${label}: this machine is on ${localInputDisplay}`);
      } else {
        // detection failed, ask the user
        p.log.warn(`${label}: could not detect current input`);
        const li = await p.select({
          message: `input on "${label}" connected to this machine (${machineName})?`,
          options: KNOWN_INPUTS.map((inp) => ({
            value: inp.value,
            label: inp.label,
            hint: inp.vcp,
          })),
        });
        if (p.isCancel(li)) {
          p.cancel("setup cancelled");
          process.exit(0);
        }
        localInputValue = li as string;
      }

      monitorSetups.push({
        name: label,
        monitorId: mon.id,
        localInput: localInputValue!,
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

  // register as start-on-boot service and start the daemon
  const daemonRole = role as string;
  const daemonBin = daemonRole === "orchestrator" ? "softkvm-orchestrator" : "softkvm-agent";
  const binPath = await findBinary(daemonBin);

  if (binPath) {
    spinner.start("registering start-on-boot service");
    const serviceOk = await registerService(daemonRole, binPath, configPath);
    spinner.stop(
      serviceOk
        ? "registered as start-on-boot service"
        : "could not register start-on-boot (can be done manually later)"
    );

    spinner.start(`starting ${daemonBin}`);
    const started = await startDaemon(binPath, configPath);
    spinner.stop(
      started
        ? `${daemonBin} is running`
        : `could not start ${daemonBin} (start it manually with: ${daemonBin} --config ${configPath})`
    );
  } else {
    p.log.warn(`${daemonBin} not found in PATH, skipping auto-start`);
    p.log.info(`start it manually: ${daemonBin} --config ${configPath}`);
  }

  // show next steps
  const notes = [];
  if (daemonRole === "orchestrator" && !serverName) {
    notes.push("run the installer on your other machine to set it up as a client");
    notes.push("it will detect this server automatically");
  }
  notes.push("run `softkvm status` to check health");
  notes.push("run `softkvm scan` to list detected monitors");
  notes.push("run `softkvm setup` to reconfigure");

  p.note(notes.join("\n"), "next steps");
  p.outro("setup complete");
}

// find a binary in PATH or known install directories
async function findBinary(name: string): Promise<string | null> {
  const os = detectOs();
  const ext = os === "windows" ? ".exe" : "";
  const fullName = name + ext;

  // check known install directories first
  const knownDirs = os === "windows"
    ? [join(process.env.LOCALAPPDATA ?? "", "softkvm", "bin")]
    : [join(process.env.HOME ?? "~", ".softkvm", "bin")];

  for (const dir of knownDirs) {
    const candidate = join(dir, fullName);
    if (existsSync(candidate)) return candidate;
  }

  // fall back to PATH lookup
  try {
    const cmd = os === "windows" ? `where ${fullName}` : `which ${fullName}`;
    const result = await $`sh -c ${cmd}`.text();
    const path = result.trim().split("\n")[0];
    if (path && existsSync(path)) return path;
  } catch {
    // not in PATH
  }

  return null;
}

// register the daemon as a start-on-boot service
async function registerService(role: string, binPath: string, configPath: string): Promise<boolean> {
  const os = detectOs();
  const serviceName = role === "orchestrator" ? "softkvm-orchestrator" : "softkvm-agent";

  try {
    if (os === "macos") {
      return await registerLaunchAgent(serviceName, binPath, configPath);
    } else if (os === "linux") {
      return await registerSystemdUser(serviceName, binPath, configPath);
    } else {
      return await registerWindowsTask(serviceName, binPath, configPath);
    }
  } catch {
    return false;
  }
}

async function registerLaunchAgent(name: string, binPath: string, configPath: string): Promise<boolean> {
  const label = `dev.softkvm.${name}`;
  const plistDir = join(process.env.HOME ?? "~", "Library", "LaunchAgents");
  const plistPath = join(plistDir, `${label}.plist`);
  const logPath = join(process.env.HOME ?? "~", "Library", "Logs", "softkvm.log");

  // unload existing service if present
  if (existsSync(plistPath)) {
    try { await $`launchctl unload ${plistPath}`.quiet(); } catch {}
  }

  if (!existsSync(plistDir)) {
    mkdirSync(plistDir, { recursive: true });
  }

  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>${binPath}</string>
        <string>--config</string>
        <string>${configPath}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${logPath}</string>
    <key>StandardErrorPath</key>
    <string>${logPath}</string>
</dict>
</plist>`;

  writeFileSync(plistPath, plist);
  await $`launchctl load ${plistPath}`.quiet();
  return true;
}

async function registerSystemdUser(name: string, binPath: string, configPath: string): Promise<boolean> {
  const unitDir = join(
    process.env.XDG_CONFIG_HOME ?? join(process.env.HOME ?? "~", ".config"),
    "systemd", "user"
  );
  const unitPath = join(unitDir, `${name}.service`);

  // stop existing service if running
  try { await $`systemctl --user stop ${name}`.quiet(); } catch {}

  if (!existsSync(unitDir)) {
    mkdirSync(unitDir, { recursive: true });
  }

  const unit = `[Unit]
Description=softkvm ${name.replace("softkvm-", "")}
After=network.target graphical-session.target

[Service]
Type=simple
ExecStart=${binPath} --config ${configPath}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
`;

  writeFileSync(unitPath, unit);
  await $`systemctl --user daemon-reload`.quiet();
  await $`systemctl --user enable ${name}`.quiet();
  return true;
}

async function registerWindowsTask(name: string, binPath: string, configPath: string): Promise<boolean> {
  // use HKCU Run key (no admin required, runs at user logon)
  const cmd = `"${binPath}" --config "${configPath}"`;
  await $`reg add "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run" /v ${name} /t REG_SZ /d ${cmd} /f`.quiet();
  return true;
}

// start the daemon as a detached background process
async function startDaemon(binPath: string, configPath: string): Promise<boolean> {
  const os = detectOs();

  try {
    if (os === "macos") {
      // launchctl already started it via load, verify it's running
      const name = binPath.includes("orchestrator") ? "softkvm-orchestrator" : "softkvm-agent";
      const label = `dev.softkvm.${name}`;
      try {
        const result = await $`launchctl list ${label}`.text();
        return result.includes(label);
      } catch {
        // launchctl list fails if not loaded, try direct spawn
        Bun.spawn([binPath, "--config", configPath], { stdout: "ignore", stderr: "ignore" });
        return true;
      }
    } else if (os === "linux") {
      const name = binPath.includes("orchestrator") ? "softkvm-orchestrator" : "softkvm-agent";
      await $`systemctl --user start ${name}`.quiet();
      return true;
    } else {
      // windows: spawn detached
      Bun.spawn([binPath, "--config", configPath], { stdout: "ignore", stderr: "ignore" });
      return true;
    }
  } catch {
    // fallback: spawn directly
    try {
      Bun.spawn([binPath, "--config", configPath], { stdout: "ignore", stderr: "ignore" });
      return true;
    } catch {
      return false;
    }
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
