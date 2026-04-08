use anyhow::Result;
use clap::Parser;
use full_kvm_core::config::Config;
use full_kvm_core::ddc::DdcController;
use tokio::sync::mpsc;

mod agent_listener;
mod deskflow;
mod discovery;
mod ipc_server;
mod key_interceptor;
mod log_parser;
mod switch_engine;

#[derive(Parser)]
#[command(name = "full-kvm-orchestrator", about = "full-kvm orchestrator daemon")]
struct Cli {
    /// path to config file
    #[arg(short, long, default_value = "full-kvm.toml")]
    config: String,

    /// skip spawning deskflow (for testing without deskflow installed)
    #[arg(long)]
    no_deskflow: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let config = Config::from_file(std::path::Path::new(&cli.config))?;

    let server_name = config
        .topology()
        .server()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "unknown".into());

    tracing::info!(
        server = server_name,
        machines = config.machines.len(),
        monitors = config.monitors.len(),
        "configuration loaded"
    );

    // start discovery responder
    let disc_name = server_name.clone();
    let disc_port = config.network.listen_port;
    tokio::spawn(async move {
        if let Err(e) = discovery::run_discovery_responder(
            disc_name,
            env!("CARGO_PKG_VERSION").to_string(),
            disc_port,
        )
        .await
        {
            tracing::warn!(error = %e, "discovery responder failed");
        }
    });

    // set up switch engine with stub DDC controller
    // on a real system this would be the ddc-hi backed controller
    let ddc: Box<dyn DdcController> = Box::new(StubController);
    let engine = switch_engine::SwitchEngine::new(config.clone());

    // build OS lookup for machines (name -> OsType)
    let machine_os: std::collections::HashMap<String, full_kvm_core::keymap::OsType> = config
        .machines
        .iter()
        .map(|m| (m.name.clone(), m.os))
        .collect();

    // determine local OS from the server machine entry
    let local_os = machine_os
        .get(&server_name)
        .copied()
        .unwrap_or(full_kvm_core::keymap::OsType::Linux);

    // initialize key interceptor for combo-aware translation
    let interceptor = key_interceptor::KeyInterceptor::new(
        local_os,
        config.keyboard.translations.clone(),
        config.keyboard.auto_remap,
    );
    if let Err(e) = interceptor.start() {
        tracing::warn!(error = %e, "key interceptor failed to start, continuing without combo translation");
    }

    // channel for deskflow log lines (either from the real process or injected for testing)
    let (log_tx, mut log_rx) = mpsc::channel::<String>(256);

    // spawn deskflow if managed
    let mut _deskflow_process: Option<deskflow::DeskflowProcess> = None;
    if config.deskflow.managed && !cli.no_deskflow {
        match deskflow::DeskflowProcess::spawn(&config).await {
            Ok((process, mut deskflow_rx)) => {
                _deskflow_process = Some(process);
                // forward deskflow output to the log channel
                let tx = log_tx.clone();
                tokio::spawn(async move {
                    while let Some(line) = deskflow_rx.recv().await {
                        if tx.send(line).await.is_err() {
                            break;
                        }
                    }
                });
                tracing::info!("deskflow-core server started");
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to start deskflow-core");
                if !cli.no_deskflow {
                    return Err(e);
                }
            }
        }
    } else {
        tracing::info!("deskflow management disabled, running without deskflow");
    }

    // drop the sender so log_rx will close when deskflow exits
    drop(log_tx);

    tracing::info!("orchestrator running");

    // main event loop
    loop {
        tokio::select! {
            // process deskflow log lines for transition events
            line = log_rx.recv() => {
                match line {
                    Some(line) => {
                        tracing::trace!(line = line, "deskflow output");
                        if let Some(event) = log_parser::parse_line(&line) {
                            tracing::info!(
                                from = event.source,
                                to = event.target,
                                x = event.x,
                                y = event.y,
                                "screen transition detected"
                            );

                            // update key interceptor with the new remote OS context
                            // if we switched away from local, the target is the remote OS
                            // if we switched back to local, clear remote OS
                            let remote_os = if event.target == server_name {
                                None // back on local machine
                            } else {
                                machine_os.get(&event.target).copied()
                            };
                            interceptor.set_remote_os(remote_os);

                            let results = engine.handle_transition(&event, ddc.as_ref());
                            for r in &results {
                                if r.success {
                                    tracing::info!(
                                        monitor = r.monitor_id,
                                        local = r.local,
                                        "monitor switched"
                                    );
                                } else if let Some(ref err) = r.error {
                                    tracing::error!(
                                        monitor = r.monitor_id,
                                        error = err,
                                        "monitor switch failed"
                                    );
                                }
                            }
                        }
                    }
                    None => {
                        tracing::warn!("deskflow log channel closed");
                        break;
                    }
                }
            }
            // handle ctrl+c
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received shutdown signal");
                break;
            }
        }
    }

    // cleanup
    if let Some(mut proc) = _deskflow_process {
        tracing::info!("stopping deskflow-core");
        let _ = proc.kill().await;
    }

    tracing::info!("orchestrator shut down");
    Ok(())
}

/// placeholder DDC controller until ddc-hi is integrated
struct StubController;

impl DdcController for StubController {
    fn enumerate_monitors(&self) -> full_kvm_core::error::Result<Vec<full_kvm_core::protocol::MonitorInfo>> {
        Ok(vec![])
    }

    fn get_input_source(&self, monitor_id: &str) -> full_kvm_core::error::Result<u16> {
        tracing::debug!(monitor = monitor_id, "stub: get_input_source");
        Err(full_kvm_core::error::FullKvmError::Ddc("stub controller".into()))
    }

    fn set_input_source(&self, monitor_id: &str, value: u16) -> full_kvm_core::error::Result<()> {
        tracing::info!(monitor = monitor_id, input = format!("0x{:02x}", value), "stub: would switch monitor input");
        Ok(())
    }
}
