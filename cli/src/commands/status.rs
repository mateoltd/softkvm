use anyhow::Result;
use softkvm_core::ddc::DdcController;

pub async fn run() -> Result<()> {
    println!("softkvm status\n");

    // check config
    let config_path = find_config();
    match &config_path {
        Some(path) => match softkvm_core::config::Config::from_file(std::path::Path::new(path)) {
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

fn find_config() -> Option<String> {
    // check current directory first
    if std::path::Path::new("softkvm.toml").exists() {
        return Some("softkvm.toml".into());
    }

    // check platform config dir
    let home = std::env::var("HOME").ok()?;
    let candidates = if cfg!(target_os = "macos") {
        vec![format!(
            "{home}/Library/Application Support/softkvm/softkvm.toml"
        )]
    } else if cfg!(target_os = "windows") {
        let local = std::env::var("LOCALAPPDATA").ok()?;
        vec![format!("{local}/softkvm/softkvm.toml")]
    } else {
        let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
        vec![format!("{xdg}/softkvm/softkvm.toml")]
    };

    candidates
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

#[cfg(feature = "stub-ddc")]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(softkvm_core::ddc::stub::StubDdcController::new())
}

#[cfg(all(not(feature = "stub-ddc"), feature = "real-ddc"))]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(softkvm_core::ddc::real::RealDdcController::new())
}

#[cfg(all(not(feature = "stub-ddc"), not(feature = "real-ddc")))]
fn create_controller() -> Box<dyn DdcController> {
    struct NullController;
    impl DdcController for NullController {
        fn enumerate_monitors(
            &self,
        ) -> softkvm_core::error::Result<Vec<softkvm_core::protocol::MonitorInfo>> {
            Ok(vec![])
        }
        fn get_input_source(&self, id: &str) -> softkvm_core::error::Result<u16> {
            Err(softkvm_core::error::SoftKvmError::Ddc(format!(
                "no DDC backend available for {id}"
            )))
        }
        fn set_input_source(&self, id: &str, _value: u16) -> softkvm_core::error::Result<()> {
            Err(softkvm_core::error::SoftKvmError::Ddc(format!(
                "no DDC backend available for {id}"
            )))
        }
    }
    Box::new(NullController)
}
