use anyhow::Result;
use softkvm_core::ddc::{switch_with_retry, DdcController};
use softkvm_core::input_source::InputSource;
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
            anyhow::bail!("monitor '{monitor_id}' not found (no DDC/CI monitors detected)");
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
    if let Some(path) = softkvm_core::config::find_config_file() {
        if let Ok(config) = softkvm_core::config::Config::from_file(std::path::Path::new(&path)) {
            return config.input_aliases;
        }
    }
    HashMap::new()
}

#[cfg(feature = "real-ddc")]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(softkvm_core::ddc::real::RealDdcController::new())
}

#[cfg(all(not(feature = "real-ddc"), feature = "stub-ddc"))]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(softkvm_core::ddc::stub::StubDdcController::new())
}

#[cfg(all(not(feature = "real-ddc"), not(feature = "stub-ddc")))]
fn create_controller() -> Box<dyn DdcController> {
    compile_error!("enable either the real-ddc or stub-ddc feature");
    Box::new(NullController)
}
