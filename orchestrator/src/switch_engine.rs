use softkvm_core::config::Config;
use softkvm_core::ddc::{switch_with_retry, DdcController};
use softkvm_core::input_source::InputSource;

use crate::log_parser::TransitionEvent;

/// handles screen transition events and dispatches DDC commands
pub struct SwitchEngine {
    config: Config,
}

impl SwitchEngine {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// process a transition event and switch monitors accordingly
    pub fn handle_transition(
        &self,
        event: &TransitionEvent,
        controller: &dyn DdcController,
    ) -> Vec<SwitchResult> {
        let topology = self.config.topology();
        let monitors = topology.monitors_for_transition(&event.target);
        let mut results = Vec::new();

        for (monitor, input_str) in monitors {
            let input = InputSource::from_str_with_aliases(input_str, &self.config.input_aliases);
            let Some(input) = input else {
                tracing::error!(
                    monitor = monitor.name,
                    input = input_str,
                    "unknown input source"
                );
                results.push(SwitchResult {
                    monitor_id: monitor.monitor_id.clone(),
                    success: false,
                    error: Some(format!("unknown input source: {input_str}")),
                    local: monitor.connected_to == self.server_name(),
                });
                continue;
            };

            let vcp = input.to_vcp_value(&self.config.input_aliases);

            // only switch monitors controlled by the local machine (server)
            if monitor.connected_to == self.server_name() {
                let result = switch_with_retry(
                    controller,
                    &monitor.monitor_id,
                    vcp,
                    self.config.ddc.skip_if_current,
                    self.config.ddc.retry_count,
                    self.config.ddc.retry_delay_ms,
                );

                results.push(SwitchResult {
                    monitor_id: monitor.monitor_id.clone(),
                    success: result.is_ok(),
                    error: result.err().map(|e| e.to_string()),
                    local: true,
                });
            } else {
                // remote monitor -- needs to be sent to the agent
                tracing::info!(
                    monitor = monitor.name,
                    target_machine = monitor.connected_to,
                    input = %input,
                    "queuing remote DDC switch"
                );
                results.push(SwitchResult {
                    monitor_id: monitor.monitor_id.clone(),
                    success: true, // will be confirmed by agent ack
                    error: None,
                    local: false,
                });
            }
        }

        results
    }

    fn server_name(&self) -> String {
        self.config
            .topology()
            .server()
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct SwitchResult {
    pub monitor_id: String,
    pub success: bool,
    pub error: Option<String>,
    /// whether this switch was executed locally or needs remote agent
    pub local: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use softkvm_core::ddc::stub::StubDdcController;

    const TEST_CONFIG: &str = r#"
[general]
role = "orchestrator"

[[machine]]
name = "Windows-PC"
role = "server"
os = "windows"

[[machine]]
name = "MacBook"
role = "client"
os = "macos"

[[monitor]]
name = "Test Monitor"
monitor_id = "TEST:MON:001"
connected_to = "Windows-PC"

[monitor.inputs]
"Windows-PC" = "DisplayPort1"
"MacBook" = "HDMI1"

[layout]
"Windows-PC" = { right = "MacBook" }
"MacBook" = { left = "Windows-PC" }
"#;

    #[test]
    fn test_handle_transition_local() {
        let config = softkvm_core::config::Config::from_toml(TEST_CONFIG).unwrap();
        let engine = SwitchEngine::new(config);
        let controller = StubDdcController::new();

        let event = TransitionEvent {
            source: "Windows-PC".into(),
            target: "MacBook".into(),
            x: 1920,
            y: 540,
        };

        let results = engine.handle_transition(&event, &controller);
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(results[0].local);

        // verify the monitor was switched to HDMI1 (0x11)
        assert_eq!(controller.get_input_source("TEST:MON:001").unwrap(), 0x11);
    }
}
