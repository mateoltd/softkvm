use anyhow::Result;
use full_kvm_core::ddc::{switch_with_retry, DdcController};
use full_kvm_core::input_source::InputSource;
use std::collections::HashMap;

pub async fn run(monitor_id: &str, input: &str) -> Result<()> {
    // resolve the input source string to a VCP value
    let aliases = load_aliases();
    let source = InputSource::from_str_with_aliases(input, &aliases)
        .ok_or_else(|| anyhow::anyhow!("unknown input source: {input}"))?;
    let vcp_value = source.to_vcp_value(&aliases);

    println!(
        "switching {} to {} (VCP 0x{:02x})",
        monitor_id, source, vcp_value
    );

    let controller = create_controller();

    // verify the monitor exists
    let monitors = controller
        .enumerate_monitors()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !monitors.iter().any(|m| m.id == monitor_id) {
        let available: Vec<&str> = monitors.iter().map(|m| m.id.as_str()).collect();
        if available.is_empty() {
            anyhow::bail!(
                "monitor '{monitor_id}' not found (no DDC/CI monitors detected)"
            );
        } else {
            anyhow::bail!(
                "monitor '{monitor_id}' not found\navailable monitors: {}",
                available.join(", ")
            );
        }
    }

    let start = std::time::Instant::now();
    match switch_with_retry(&*controller, monitor_id, vcp_value, true, 3, 50) {
        Ok(switched) => {
            let elapsed = start.elapsed();
            if switched {
                println!(
                    "switched to {} in {:.0}ms",
                    source,
                    elapsed.as_secs_f64() * 1000.0
                );
            } else {
                println!("monitor already on {}, no switch needed", source);
            }
        }
        Err(e) => {
            anyhow::bail!("DDC switch failed after 3 attempts: {e}");
        }
    }

    Ok(())
}

/// load input aliases from config if available, otherwise empty
fn load_aliases() -> HashMap<String, u16> {
    // try to find and load the config for aliases
    let config_paths = [
        dirs_for_config("full-kvm.toml"),
        Some("full-kvm.toml".to_string()),
    ];

    for path in config_paths.into_iter().flatten() {
        let p = std::path::Path::new(&path);
        if p.exists() {
            if let Ok(config) = full_kvm_core::config::Config::from_file(p) {
                return config.input_aliases;
            }
        }
    }

    HashMap::new()
}

/// platform-specific config directory
fn dirs_for_config(filename: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    if cfg!(target_os = "macos") {
        Some(format!(
            "{home}/Library/Application Support/full-kvm/{filename}"
        ))
    } else if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA")
            .ok()
            .map(|d| format!("{d}/full-kvm/{filename}"))
    } else {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{home}/.config"));
        Some(format!("{xdg}/full-kvm/{filename}"))
    }
}

#[cfg(feature = "stub-ddc")]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(full_kvm_core::ddc::stub::StubDdcController::new())
}

#[cfg(all(not(feature = "stub-ddc"), feature = "real-ddc"))]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(full_kvm_core::ddc::real::RealDdcController::new())
}

#[cfg(all(not(feature = "stub-ddc"), not(feature = "real-ddc")))]
fn create_controller() -> Box<dyn DdcController> {
    struct NullController;
    impl DdcController for NullController {
        fn enumerate_monitors(&self) -> full_kvm_core::error::Result<Vec<full_kvm_core::protocol::MonitorInfo>> {
            Ok(vec![])
        }
        fn get_input_source(&self, id: &str) -> full_kvm_core::error::Result<u16> {
            Err(full_kvm_core::error::FullKvmError::Ddc(format!("no DDC backend available for {id}")))
        }
        fn set_input_source(&self, id: &str, _value: u16) -> full_kvm_core::error::Result<()> {
            Err(full_kvm_core::error::FullKvmError::Ddc(format!("no DDC backend available for {id}")))
        }
    }
    Box::new(NullController)
}
