import * as p from "@clack/prompts";
import { platform, hostname } from "os";
import { writeFileSync, mkdirSync, existsSync } from "fs";
import { join } from "path";
import { execSync, spawn } from "child_process";
import { discoverServers, type ServerInfo } from "./discover";
import { scanMonitors, KNOWN_INPUTS, detectedInputConfigValue, detectedInputLabel, monitorLabel, monitorHint, type MonitorInfo } from "./monitor-scan";
import { generateConfig, type SetupAnswers, type MonitorSetup } from "./config-gen";

// shell helper replacing Bun's $ tagged template (Node-compatible)
function exec(cmd: string): string {
  return execSync(cmd, { encoding: "utf-8", stdio: ["pipe", "pipe", "pipe"] }).trim();
}
function execQuiet(cmd: string): void {
  execSync(cmd, { stdio: "ignore" });
}
function spawnDetached(bin: string, args: string[]): void {
  const child = spawn(bin, args, { detached: true, stdio: "ignore" });
  child.unref();
}

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
    execQuiet(`softkvm identify ${monitorId}`);
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

// check if deskflow-core is available (in PATH or known locations)
function findDeskflowCore(): string | null {
  // check PATH
  try {
    const cmd = detectOs() === "windows" ? "where deskflow-core" : "which deskflow-core";
    return exec(cmd).split("\n")[0] || null;
  } catch {}

  // check known install locations
  const os = detectOs();
  const candidates: string[] = [];
  if (os === "macos") {
    candidates.push("/Applications/Deskflow.app/Contents/MacOS/deskflow-core");
  } else if (os === "windows") {
    const pf = process.env.ProgramFiles ?? "C:\\Program Files";
    candidates.push(join(pf, "Deskflow", "deskflow-core.exe"));
  } else {
    candidates.push("/usr/bin/deskflow-core", "/usr/local/bin/deskflow-core");
  }

  for (const c of candidates) {
    if (existsSync(c)) return c;
  }
  return null;
}

async function main() {
  p.intro("softkvm setup");

  // check dependencies
  const deskflowPath = findDeskflowCore();
  if (!deskflowPath) {
    const os = detectOs();
    p.log.warn("deskflow-core not found (required for mouse/keyboard sharing)");
    if (os === "macos") {
      p.log.info("install: brew install --cask deskflow/tap/deskflow");
    } else if (os === "windows") {
      p.log.info("install: winget install --id=Deskflow.Deskflow -e");
    } else {
      p.log.info("install via your package manager, or download from:");
      p.log.info("https://github.com/deskflow/deskflow/releases");
    }

    const cont = await p.confirm({
      message: "continue setup without deskflow? (DDC switching will work, but no mouse/keyboard sharing)",
      initialValue: true,
    });
    if (p.isCancel(cont) || !cont) {
      p.cancel("install deskflow first, then re-run softkvm setup");
      process.exit(0);
    }
  } else {
    p.log.success(`deskflow-core found: ${deskflowPath}`);
  }

  // macOS requires explicit permissions for deskflow to control keyboard/mouse
  if (detectOs() === "macos") {
    p.note(
      "deskflow needs macOS permissions to share keyboard and mouse:\n\n" +
      "  1. System Settings > Privacy & Security > Accessibility\n" +
      "     add deskflow-core (or Deskflow.app)\n\n" +
      "  2. System Settings > Privacy & Security > Input Monitoring\n" +
      "     add deskflow-core (or Deskflow.app)\n\n" +
      "without these, keyboard and mouse sharing will not work.",
      "macOS permissions"
    );
  }

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
  let remoteOs: "windows" | "macos" | "linux" | undefined;

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

    // ask remote OS for the server machine
    const ros = await p.select({
      message: `what OS does "${serverName}" run?`,
      options: [
        { value: "windows", label: "Windows" },
        { value: "macos", label: "macOS" },
        { value: "linux", label: "Linux" },
      ],
    });
    if (p.isCancel(ros)) {
      p.cancel("setup cancelled");
      process.exit(0);
    }
    remoteOs = ros as "windows" | "macos" | "linux";
  } else {
    // server role: client name is optional, agents identify themselves on connect
    const wantClient = await p.confirm({
      message: "do you want to configure a client machine now? (can be added later when it connects)",
      initialValue: true,
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

      const ros = await p.select({
        message: `what OS does "${name}" run?`,
        options: [
          { value: "windows", label: "Windows" },
          { value: "macos", label: "macOS" },
          { value: "linux", label: "Linux" },
        ],
      });
      if (p.isCancel(ros)) {
        p.cancel("setup cancelled");
        process.exit(0);
      }
      remoteOs = ros as "windows" | "macos" | "linux";
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

      // remote input: ask which input the other machine is on
      let remoteInputValue: string | undefined;
      if (serverName) {
        const ri = await p.select({
          message: `input on "${label}" connected to "${serverName}"?`,
          options: KNOWN_INPUTS.map((inp) => ({
            value: inp.value,
            label: inp.label,
            hint: inp.vcp,
          })),
        });
        if (p.isCancel(ri)) {
          p.cancel("setup cancelled");
          process.exit(0);
        }
        remoteInputValue = ri as string;
      }

      monitorSetups.push({
        name: label,
        monitorId: mon.id,
        localInput: localInputValue!,
        remoteInput: remoteInputValue,
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
    remoteOs,
    serverName,
    serverAddress,
    monitors: monitorSetups,
    layout: direction && serverName
      ? { direction: direction as "left" | "right" | "up" | "down", neighborName: serverName }
      : undefined,
    deskflowPath: deskflowPath ?? undefined,
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
  process.exit(0);
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
    const result = exec(cmd);
    const path = result.split("\n")[0];
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
    try { execQuiet(`launchctl unload ${plistPath}`); } catch {}
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
  execQuiet(`launchctl load ${plistPath}`);
  return true;
}

async function registerSystemdUser(name: string, binPath: string, configPath: string): Promise<boolean> {
  const unitDir = join(
    process.env.XDG_CONFIG_HOME ?? join(process.env.HOME ?? "~", ".config"),
    "systemd", "user"
  );
  const unitPath = join(unitDir, `${name}.service`);

  // stop existing service if running
  try { execQuiet(`systemctl --user stop ${name}`); } catch {}

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
  execQuiet("systemctl --user daemon-reload");
  execQuiet(`systemctl --user enable ${name}`);
  return true;
}

async function registerWindowsTask(name: string, binPath: string, configPath: string): Promise<boolean> {
  // use HKCU Run key (no admin required, runs at user logon)
  const val = `\\"${binPath}\\" --config \\"${configPath}\\"`;
  execQuiet(`reg add "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run" /v ${name} /t REG_SZ /d "${val}" /f`);

  // add firewall rules for discovery (UDP), agent connections (TCP), and deskflow (TCP)
  try {
    execQuiet(`netsh advfirewall firewall add rule name="softkvm discovery" dir=in action=allow protocol=UDP localport=24802`);
    execQuiet(`netsh advfirewall firewall add rule name="softkvm agent" dir=in action=allow protocol=TCP localport=24801`);
    execQuiet(`netsh advfirewall firewall add rule name="softkvm deskflow" dir=in action=allow protocol=TCP localport=24800`);
  } catch (e) {
    p.log.warn(`could not add firewall rules (requires admin): ${e}`);
    p.log.info("run as administrator or manually allow TCP 24800, TCP 24801, UDP 24802");
  }
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
        const result = exec(`launchctl list ${label}`);
        return result.includes(label);
      } catch {
        // launchctl list fails if not loaded, try direct spawn
        spawnDetached(binPath, ["--config", configPath]);
        return true;
      }
    } else if (os === "linux") {
      const name = binPath.includes("orchestrator") ? "softkvm-orchestrator" : "softkvm-agent";
      execQuiet(`systemctl --user start ${name}`);
      return true;
    } else {
      // windows: spawn detached
      spawnDetached(binPath, ["--config", configPath]);
      return true;
    }
  } catch {
    // fallback: spawn directly
    try {
      spawnDetached(binPath, ["--config", configPath]);
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
