use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Result, SoftKvmError};
use crate::keymap::KeyboardConfig;
use crate::topology::{LayoutLinks, MachineConfig, MachineRole, MonitorConfig, Topology};

/// Top-level configuration, deserialized from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,

    #[serde(default)]
    pub deskflow: DeskflowConfig,

    #[serde(default)]
    pub network: NetworkConfig,

    #[serde(default, rename = "machine")]
    pub machines: Vec<MachineConfig>,

    #[serde(default, rename = "monitor")]
    pub monitors: Vec<MonitorConfig>,

    #[serde(default)]
    pub input_aliases: HashMap<String, u16>,

    #[serde(default)]
    pub layout: HashMap<String, LayoutLinks>,

    #[serde(default)]
    pub keyboard: KeyboardConfig,

    #[serde(default)]
    pub behavior: BehaviorConfig,

    #[serde(default)]
    pub ddc: DdcConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub role: RoleConfig,

    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoleConfig {
    Orchestrator,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeskflowConfig {
    #[serde(default = "default_true")]
    pub managed: bool,

    #[serde(default = "default_deskflow_binary")]
    pub binary_path: String,

    #[serde(default = "default_switch_delay")]
    pub switch_delay: u32,

    #[serde(default)]
    pub switch_double_tap: u32,

    #[serde(default = "default_true")]
    pub clipboard_sharing: bool,

    #[serde(default = "default_clipboard_size")]
    pub clipboard_max_size_kb: u32,
}

impl Default for DeskflowConfig {
    fn default() -> Self {
        Self {
            managed: true,
            binary_path: default_deskflow_binary(),
            switch_delay: default_switch_delay(),
            switch_double_tap: 0,
            clipboard_sharing: true,
            clipboard_max_size_kb: default_clipboard_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,

    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    #[serde(default = "default_true")]
    pub tls: bool,

    /// Agent-only: address of the orchestrator.
    /// Set to "auto" for UDP discovery.
    pub orchestrator_address: Option<String>,

    pub orchestrator_port: Option<u16>,

    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval_ms: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_port: default_listen_port(),
            listen_address: default_listen_address(),
            tls: true,
            orchestrator_address: None,
            orchestrator_port: None,
            reconnect_interval_ms: default_reconnect_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorConfig {
    #[serde(default = "default_focus_lock_hotkey")]
    pub focus_lock_hotkey: String,

    #[serde(default = "default_quick_switch_hotkey")]
    pub quick_switch_hotkey: String,

    #[serde(default = "default_quick_switch_back_hotkey")]
    pub quick_switch_back_hotkey: String,

    #[serde(default = "default_true")]
    pub adaptive_switch_delay: bool,

    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_min: u32,

    #[serde(default = "default_true")]
    pub toast_notifications: bool,

    #[serde(default = "default_toast_duration")]
    pub toast_duration_ms: u32,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            focus_lock_hotkey: default_focus_lock_hotkey(),
            quick_switch_hotkey: default_quick_switch_hotkey(),
            quick_switch_back_hotkey: default_quick_switch_back_hotkey(),
            adaptive_switch_delay: true,
            idle_timeout_min: default_idle_timeout(),
            toast_notifications: true,
            toast_duration_ms: default_toast_duration(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DdcConfig {
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,

    #[serde(default = "default_retry_delay")]
    pub retry_delay_ms: u64,

    #[serde(default = "default_inter_command_delay")]
    pub inter_command_delay_ms: u64,

    #[serde(default = "default_wake_delay")]
    pub wake_delay_ms: u64,

    #[serde(default = "default_true")]
    pub skip_if_current: bool,
}

impl Default for DdcConfig {
    fn default() -> Self {
        Self {
            retry_count: default_retry_count(),
            retry_delay_ms: default_retry_delay(),
            inter_command_delay_ms: default_inter_command_delay(),
            wake_delay_ms: default_wake_delay(),
            skip_if_current: true,
        }
    }
}

// --- Default value functions ---

fn default_true() -> bool {
    true
}
fn default_log_level() -> String {
    "info".into()
}
fn default_deskflow_binary() -> String {
    "deskflow-core".into()
}
fn default_switch_delay() -> u32 {
    250
}
fn default_clipboard_size() -> u32 {
    1024
}
fn default_listen_port() -> u16 {
    24801
}
fn default_listen_address() -> String {
    "0.0.0.0".into()
}
fn default_reconnect_interval() -> u64 {
    3000
}
fn default_focus_lock_hotkey() -> String {
    "ScrollLock".into()
}
fn default_quick_switch_hotkey() -> String {
    "ctrl+alt+right".into()
}
fn default_quick_switch_back_hotkey() -> String {
    "ctrl+alt+left".into()
}
fn default_idle_timeout() -> u32 {
    30
}
fn default_toast_duration() -> u32 {
    500
}
fn default_retry_count() -> u32 {
    3
}
fn default_retry_delay() -> u64 {
    50
}
fn default_inter_command_delay() -> u64 {
    40
}
fn default_wake_delay() -> u64 {
    3000
}

// --- Validation ---

impl Config {
    /// Load config from a TOML string.
    pub fn from_toml(s: &str) -> Result<Self> {
        let config: Config = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }

    /// Load config from a file path.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_toml(&contents)
    }

    /// Load config, searching platform-specific directories if the given
    /// path doesn't exist. Returns the resolved path alongside the config.
    pub fn load_or_find(path: &str) -> Result<(Self, String)> {
        let p = std::path::Path::new(path);
        if p.exists() {
            let config = Self::from_file(p)?;
            return Ok((config, path.to_string()));
        }

        // if the caller passed a custom path (not the default), don't search
        if path != "softkvm.toml" {
            return Err(SoftKvmError::Config(format!(
                "config file not found: {path}"
            )));
        }

        // search platform config directories
        if let Some(found) = find_config_file() {
            let config = Self::from_file(std::path::Path::new(&found))?;
            Ok((config, found))
        } else {
            Err(SoftKvmError::Config(
                "config file not found. run `softkvm setup` to create one".into(),
            ))
        }
    }

    /// Validate referential integrity of the configuration.
    pub fn validate(&self) -> Result<()> {
        // Check exactly one server
        let server_count = self
            .machines
            .iter()
            .filter(|m| m.role == MachineRole::Server)
            .count();
        if server_count != 1 {
            return Err(SoftKvmError::Config(format!(
                "expected exactly 1 server machine, found {server_count}"
            )));
        }

        // Check unique machine names
        let mut names = std::collections::HashSet::new();
        for machine in &self.machines {
            if !names.insert(&machine.name) {
                return Err(SoftKvmError::Config(format!(
                    "duplicate machine name: {}",
                    machine.name
                )));
            }
        }

        // Check monitor references
        for monitor in &self.monitors {
            if !names.contains(&monitor.connected_to) {
                return Err(SoftKvmError::Config(format!(
                    "monitor '{}' references unknown machine '{}'",
                    monitor.name, monitor.connected_to
                )));
            }
            for machine_name in monitor.inputs.keys() {
                if !names.contains(machine_name) {
                    return Err(SoftKvmError::Config(format!(
                        "monitor '{}' has input mapping for unknown machine '{}'",
                        monitor.name, machine_name
                    )));
                }
            }
        }

        // Check layout references
        for (machine_name, links) in &self.layout {
            if !names.contains(machine_name) {
                return Err(SoftKvmError::Config(format!(
                    "layout references unknown machine '{machine_name}'"
                )));
            }
            for neighbor in [&links.left, &links.right, &links.up, &links.down]
                .into_iter()
                .flatten()
            {
                if !names.contains(neighbor) {
                    return Err(SoftKvmError::Config(format!(
                        "layout for '{machine_name}' references unknown machine '{neighbor}'"
                    )));
                }
            }
        }

        Ok(())
    }

    /// Build a resolved Topology from this config.
    pub fn topology(&self) -> Topology {
        Topology {
            machines: self.machines.clone(),
            monitors: self.monitors.clone(),
            layout: self.layout.clone(),
        }
    }
}

/// Search platform-specific config directories for softkvm.toml.
/// Checks current dir first, then OS-appropriate config paths.
pub fn find_config_file() -> Option<String> {
    // current directory
    if std::path::Path::new("softkvm.toml").exists() {
        return Some("softkvm.toml".into());
    }

    let candidates = platform_config_paths("softkvm.toml");
    candidates
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

/// Return platform-specific config directory paths for a given filename.
pub fn platform_config_paths(filename: &str) -> Vec<String> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            paths.push(format!(
                "{home}/Library/Application Support/softkvm/{filename}"
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(format!("{local}\\softkvm\\{filename}"));
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            paths.push(format!("{home}\\AppData\\Local\\softkvm\\{filename}"));
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            paths.push(format!("{xdg}/softkvm/{filename}"));
        }
        if let Ok(home) = std::env::var("HOME") {
            paths.push(format!("{home}/.config/softkvm/{filename}"));
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CONFIG: &str = r#"
[general]
role = "orchestrator"
log_level = "info"

[[machine]]
name = "Windows-PC"
role = "server"
os = "windows"

[[machine]]
name = "MacBook"
role = "client"
os = "macos"

[[monitor]]
name = "Dell U2720Q"
monitor_id = "DEL:U2720Q:SN12345"
connected_to = "Windows-PC"

[monitor.inputs]
"Windows-PC" = "DisplayPort1"
"MacBook" = "HDMI1"

[layout]
"Windows-PC" = { right = "MacBook" }
"MacBook" = { left = "Windows-PC" }

[ddc]
retry_count = 3
"#;

    #[test]
    fn test_parse_valid_config() {
        let config = Config::from_toml(VALID_CONFIG).unwrap();
        assert_eq!(config.machines.len(), 2);
        assert_eq!(config.monitors.len(), 1);
        assert!(config.keyboard.auto_remap);
        assert_eq!(config.ddc.retry_count, 3);
    }

    #[test]
    fn test_duplicate_machine_name() {
        let toml = r#"
[general]
role = "orchestrator"

[[machine]]
name = "PC"
role = "server"
os = "windows"

[[machine]]
name = "PC"
role = "client"
os = "macos"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("duplicate machine name"));
    }

    #[test]
    fn test_no_server() {
        let toml = r#"
[general]
role = "orchestrator"

[[machine]]
name = "PC"
role = "client"
os = "windows"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("expected exactly 1 server"));
    }

    #[test]
    fn test_monitor_references_unknown_machine() {
        let toml = r#"
[general]
role = "orchestrator"

[[machine]]
name = "PC"
role = "server"
os = "windows"

[[monitor]]
name = "Monitor"
monitor_id = "test"
connected_to = "NonExistent"

[monitor.inputs]
"PC" = "HDMI1"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("unknown machine"));
    }

    #[test]
    fn test_topology_monitors_for_transition() {
        let config = Config::from_toml(VALID_CONFIG).unwrap();
        let topo = config.topology();
        let monitors = topo.monitors_for_transition("MacBook");
        assert_eq!(monitors.len(), 1);
        assert_eq!(monitors[0].1, "HDMI1");
    }

    #[test]
    fn test_default_keyboard_translations() {
        let config = Config::from_toml(VALID_CONFIG).unwrap();
        assert!(!config.keyboard.translations.is_empty());
        assert_eq!(config.keyboard.translations[0].intent, "app_switcher");
    }
}
