# softkvm

Software-only KVM that requires **zero additional hardware**. Combines [Deskflow](https://github.com/deskflow/deskflow) (mouse, keyboard, clipboard sharing) with DDC/CI (monitor input switching over existing video cables) to create a seamless multi-machine desktop.

Move your cursor across a screen edge and the shared monitor physically switches its input source to show the target machine. Keyboard shortcuts translate automatically between macOS and Windows (Cmd+C becomes Ctrl+C, Cmd+Tab becomes Alt+Tab, etc.).

## Install

One command. Downloads binaries, registers the CLI, scans your monitors, and walks you through configuration.

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/mateoltd/softkvm/main/install/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/mateoltd/softkvm/main/install/install.ps1 | iex
```

The installer will:
1. Detect your platform and architecture
2. Download pre-built binaries (or build from source if no release exists yet)
3. Install to `~/.softkvm/bin` (macOS/Linux) or `%LOCALAPPDATA%\softkvm\bin` (Windows)
4. Add the CLI to your PATH
5. Scan connected monitors for DDC/CI support
6. Launch the interactive setup wizard

After install, open a new terminal and the `softkvm` command is available globally.

## What the setup wizard does

The installer automatically runs the setup wizard after installing. It handles everything:

```
▸ scanning network for existing softkvm servers
▸ found 1 server(s): Windows-PC (192.168.1.100)

◆ what role should this machine play?
│ ● Client (recommended) — server detected on your network
│ ○ Server

◆ machine name?
│ MacBook

◆ scanning monitors for DDC/CI support
▸ found 2 monitor(s)

◆ which input on "Dell U2720Q" corresponds to this machine (MacBook)?
│ HDMI1

◆ which input corresponds to "Windows-PC"?
│ DisplayPort1

◆ where is "Windows-PC" relative to this machine?
│ ● Left

▸ configuration written to ~/.config/softkvm/softkvm.toml
```

No manual config editing needed. Run `softkvm setup` anytime to reconfigure.

## Start the daemons

On the **primary machine** (orchestrator):

```bash
softkvm-orchestrator
```

On each **secondary machine** (agent):

```bash
softkvm-agent
```

That's it. Move your mouse across screen edges and everything switches automatically.

## How it works

```
Machine A (orchestrator)              Machine B (agent)
┌──────────────────────┐              ┌──────────────────────┐
│  deskflow server     │   network    │  deskflow client     │
│  orchestrator daemon │◄────────────►│  agent daemon        │
│  DDC/CI control      │              │  DDC/CI control      │
└────────┬─────────────┘              └────────┬─────────────┘
         │ video cable                         │ video cable
         └──────────────┐  ┌───────────────────┘
                     ┌──▼──▼──┐
                     │Monitor │  DDC/CI switches input
                     └────────┘
```

1. Deskflow handles mouse/keyboard/clipboard sharing over the network
2. The orchestrator parses Deskflow's transition logs in real time
3. On screen switch, DDC/CI commands travel over the existing video cables to change the monitor's input source
4. Keyboard shortcuts are translated between OS conventions automatically

## CLI reference

```
softkvm scan               detect monitors with DDC/CI support
softkvm scan --json        machine-readable output
softkvm switch <id> <in>   manually switch a monitor's input
softkvm validate           check config file for errors
softkvm status             show system health and config
softkvm setup              re-run the interactive setup wizard
softkvm update             check for updates
```

Daemons:

```
softkvm-orchestrator                     start the server daemon
softkvm-orchestrator --config path.toml  use a specific config
softkvm-orchestrator --no-deskflow       skip spawning deskflow

softkvm-agent                            start the client daemon
softkvm-agent --server 192.168.1.100     override server address
softkvm-agent --config path.toml         use a specific config
```

## Default hotkeys

| Shortcut | Action |
|----------|--------|
| `Ctrl+Alt+Right` | Switch to next machine |
| `Ctrl+Alt+Left` | Switch to previous machine |
| `Scroll Lock` | Toggle focus lock (prevents accidental edge switches) |

## Keyboard remapping

When `auto_remap = true` (the default), modifier keys are translated automatically:

| Mac keyboard on Windows | Windows keyboard on Mac |
|------------------------|------------------------|
| Cmd -> Ctrl | Ctrl -> Cmd |
| Option -> Alt | Alt -> Option |
| Control -> Win | Win -> Control |

Combo-aware translations handle shortcuts that differ structurally:

| Intent | Mac | Windows |
|--------|-----|---------|
| App switcher | Cmd+Tab | Alt+Tab |
| Quit app | Cmd+Q | Alt+F4 |
| Search/Spotlight | Cmd+Space | Win+S |
| Screenshot | Cmd+Shift+4 | Win+Shift+S |

Add custom translations in config:

```toml
[[keyboard.translations]]
intent = "my_shortcut"
mac = "meta+shift+k"
windows = "ctrl+shift+k"
```

## Monitor compatibility

DDC/CI must be enabled on your monitor (usually under OSD > DDC/CI). Most modern monitors support it.

Known issues:
- Mac Mini HDMI on Apple Silicon has no DDC support -- use USB-C or DisplayPort
- Some USB-C monitors use non-standard VCP values -- use `softkvm scan` to discover them

## Prerequisites

- **Deskflow** -- install from [deskflow.org](https://deskflow.org) or your package manager
- Two machines connected to the same monitor(s) via DP, HDMI, or USB-C
- Both machines on the same local network

On Linux, you may need i2c access for DDC:

```bash
sudo usermod -aG i2c $USER
```

## Build from source

Requires Rust (stable). The installer handles this automatically, but to do it manually:

```bash
git clone https://github.com/mateoltd/softkvm.git
cd softkvm
cargo build --release --features real-ddc --no-default-features
```

Binaries land in `target/release/`:
- `softkvm` -- CLI
- `softkvm-orchestrator` -- server daemon
- `softkvm-agent` -- client daemon

Copy them to a directory on your PATH, then run `softkvm setup`.

For development builds (stub DDC controller, no hardware required):

```bash
cargo build
cargo test --workspace   # 89 tests
```

## Config file locations

The CLI and daemons search for `softkvm.toml` in order:

1. Current working directory
2. `~/.config/softkvm/` (Linux)
3. `~/Library/Application Support/softkvm/` (macOS)
4. `%LOCALAPPDATA%\softkvm\` (Windows)

Or pass `--config <path>` explicitly.

## Project structure

```
softkvm/
  core/           shared library (config, protocol, DDC, keymap, topology)
  orchestrator/   server daemon (Deskflow lifecycle, log parsing, switch engine)
  agent/          client daemon (connects to orchestrator, executes DDC commands)
  cli/            command-line tool (scan, switch, validate, status, setup)
  ui/             Electron control panel (React + Tailwind)
  setup/          interactive setup TUI (Bun + @clack/prompts)
  install/        one-liner install scripts (bash, PowerShell)
```

## Full config reference

<details>
<summary>All options with defaults</summary>

```toml
[general]
role = "orchestrator"           # "orchestrator" or "agent"
log_level = "info"              # "trace", "debug", "info", "warn", "error"

[deskflow]
managed = true                  # auto-start deskflow process
binary_path = "deskflow-core"   # path to deskflow binary
switch_delay = 250              # ms cursor must stay at edge before switching
switch_double_tap = 0           # ms double-tap window (0 = disabled)
clipboard_sharing = true        # share clipboard between machines
clipboard_max_size_kb = 1024    # max clipboard payload size

[network]
listen_port = 24801             # orchestrator listen port
listen_address = "0.0.0.0"     # orchestrator bind address
tls = true                      # encrypt agent connections
orchestrator_address = "auto"   # agent: orchestrator IP or "auto" for discovery
orchestrator_port = 24801       # agent: orchestrator port
reconnect_interval_ms = 3000    # agent: base reconnect delay

[[machine]]
name = "Windows-PC"
role = "server"                 # exactly one "server", rest are "client"
os = "windows"                  # "windows", "macos", "linux"

[[monitor]]
name = "Dell U2720Q"
monitor_id = "DEL:U2720Q:ABC123"
connected_to = "Windows-PC"     # which machine physically drives DDC

[monitor.inputs]                # input source per machine
"Windows-PC" = "DisplayPort1"
"MacBook" = "HDMI1"

[layout]                        # spatial arrangement
"Windows-PC" = { right = "MacBook" }
"MacBook" = { left = "Windows-PC" }

[input_aliases]                 # custom VCP values for non-standard monitors
"USB-C" = 0x0f

[keyboard]
auto_remap = true               # auto-translate modifiers between OS pairs

[[keyboard.translations]]       # combo-aware shortcut translations
intent = "app_switcher"
mac = "meta+tab"
windows = "alt+tab"

[behavior]
focus_lock_hotkey = "ScrollLock"
quick_switch_hotkey = "ctrl+alt+right"
quick_switch_back_hotkey = "ctrl+alt+left"
adaptive_switch_delay = true    # auto-adjust delay based on usage patterns
idle_timeout_min = 30           # consider machine idle after N minutes
toast_notifications = true      # show overlay on machine switch
toast_duration_ms = 500

[ddc]
retry_count = 3                 # retries on DDC command failure
retry_delay_ms = 50             # delay between retries
inter_command_delay_ms = 40     # delay between sequential DDC commands
wake_delay_ms = 3000            # delay after waking a sleeping monitor
skip_if_current = true          # skip switch if already on target input
```

</details>

## License

MIT
