use anyhow::Result;
use softkvm_core::config::find_config_file;
use softkvm_core::ddc::DdcController;

pub async fn run() -> Result<()> {
    println!("softkvm status\n");

    // check config
    match find_config_file() {
        Some(path) => match softkvm_core::config::Config::from_file(std::path::Path::new(&path)) {
            Ok(config) => {
                let server = config
                    .topology()
                    .server()
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "unknown".into());

                println!("  config:     {path}");
                let role = match config.general.role {
                    softkvm_core::config::RoleConfig::Orchestrator => "orchestrator",
                    softkvm_core::config::RoleConfig::Agent => "agent",
                };
                println!("  role:       {role}");
                println!("  server:     {server}");
                println!("  machines:   {}", config.machines.len());
                println!("  monitors:   {}", config.monitors.len());
            }
            Err(e) => {
                println!("  config:     {path} (invalid: {e})");
            }
        },
        None => {
            println!("  config:     not found");
            println!("              run `softkvm setup` to create one");
        }
    }

    println!();

    // check DDC monitors
    let controller = create_controller();
    match controller.enumerate_monitors() {
        Ok(monitors) => {
            if monitors.is_empty() {
                println!("  DDC/CI:     no monitors detected");
            } else {
                println!("  DDC/CI:     {} monitor(s)", monitors.len());
                for m in &monitors {
                    let input = m
                        .current_input_vcp
                        .map(|v| format!("0x{v:02x}"))
                        .unwrap_or_else(|| "?".into());
                    let health = if m.ddc_supported { "ok" } else { "unsupported" };
                    println!(
                        "              {} [{}] input={} ddc={}",
                        m.name, m.id, input, health
                    );
                }
            }
        }
        Err(e) => {
            println!("  DDC/CI:     error ({e})");
        }
    }

    println!();

    // check if orchestrator is running (try to connect to IPC socket)
    // for now we just check if the process exists
    println!("  daemon:     not connected");
    println!("              (IPC socket not yet implemented)");

    Ok(())
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
