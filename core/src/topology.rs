use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::keymap::OsType;

/// Role a machine plays in the full-kvm network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MachineRole {
    Server,
    Client,
}

/// A machine in the KVM setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfig {
    /// Name of this machine. Must match the Deskflow screen name exactly.
    pub name: String,
    /// Whether this machine runs the orchestrator (server) or agent (client).
    pub role: MachineRole,
    /// Operating system running on this machine.
    pub os: OsType,
}

/// A physical monitor shared between machines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Human-readable label.
    pub name: String,
    /// DDC/CI identification string (manufacturer:model:serial from ddc-hi).
    pub monitor_id: String,
    /// Machine name that physically controls this monitor's DDC/CI.
    /// DDC commands for this monitor are sent from this machine.
    pub connected_to: String,
    /// Input source mapping: machine name -> input source string.
    /// e.g., { "Windows-PC": "DisplayPort1", "MacBook": "HDMI1" }
    pub inputs: HashMap<String, String>,
}

/// Spatial relationships between screens.
/// Each key is a machine name, value describes what's on each edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutLinks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down: Option<String>,
}

/// Resolved topology: all machines, monitors, and their relationships.
#[derive(Debug)]
pub struct Topology {
    pub machines: Vec<MachineConfig>,
    pub monitors: Vec<MonitorConfig>,
    pub layout: HashMap<String, LayoutLinks>,
}

impl Topology {
    /// Find the server machine.
    pub fn server(&self) -> Option<&MachineConfig> {
        self.machines.iter().find(|m| m.role == MachineRole::Server)
    }

    /// Find a machine by name.
    pub fn machine(&self, name: &str) -> Option<&MachineConfig> {
        self.machines.iter().find(|m| m.name == name)
    }

    /// Find all monitors that need switching when transitioning to a target machine.
    /// Returns (monitor_config, target_input_source_string) pairs.
    pub fn monitors_for_transition<'a>(&'a self, target_machine: &str) -> Vec<(&'a MonitorConfig, &'a str)> {
        self.monitors
            .iter()
            .filter_map(|mon| {
                mon.inputs
                    .get(target_machine)
                    .map(|input| (mon, input.as_str()))
            })
            .collect()
    }

    /// Find monitors controlled by a specific machine (for DDC execution routing).
    pub fn monitors_controlled_by<'a>(&'a self, machine_name: &str) -> Vec<&'a MonitorConfig> {
        self.monitors
            .iter()
            .filter(|m| m.connected_to == machine_name)
            .collect()
    }
}
