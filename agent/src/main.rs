use anyhow::Result;
use clap::Parser;
use softkvm_core::config::Config;
use softkvm_core::ddc::DdcController;
use softkvm_core::protocol::Message;
use softkvm_core::topology::MachineRole;

mod connection;

#[derive(Parser)]
#[command(name = "softkvm-agent", about = "softkvm client agent daemon")]
struct Cli {
    /// path to config file
    #[arg(short, long, default_value = "softkvm.toml")]
    config: String,

    /// orchestrator address (overrides config)
    #[arg(long)]
    server: Option<String>,
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
    let (config, config_path) =
        Config::load_or_find(&cli.config).map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::info!(config = config_path, "using config");

    // determine agent name from config
    let agent_name = config
        .machines
        .iter()
        .find(|m| m.role == MachineRole::Client)
        .map(|m| m.name.clone())
        .unwrap_or_else(hostname);

    // determine orchestrator address
    let server_addr = cli
        .server
        .or_else(|| {
            config
                .network
                .orchestrator_address
                .as_deref()
                .filter(|a| *a != "auto")
                .map(|a| format!("{a}:{}", config.network.listen_port))
        })
        .unwrap_or_else(|| format!("127.0.0.1:{}", config.network.listen_port));

    tracing::info!(
        agent = agent_name,
        server = server_addr,
        "softkvm agent starting"
    );

    let addr: std::net::SocketAddr = server_addr.parse()?;
    let ddc: Box<dyn DdcController> = create_controller();

    // connect with retry
    let mut conn = connection::connect_with_retry(addr, &agent_name, 3000, 30000).await;

    // send initial monitor inventory
    let monitors = ddc.enumerate_monitors().unwrap_or_default();
    tracing::info!(count = monitors.len(), "sending monitor inventory");
    conn.send(&Message::MonitorInventory {
        monitors: monitors.clone(),
    })
    .await?;

    // heartbeat interval
    let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(10));

    tracing::info!("agent running");

    // main event loop
    loop {
        tokio::select! {
            // receive commands from orchestrator
            msg = conn.recv() => {
                match msg {
                    Ok(Message::SwitchMonitor { monitor_id, input_source_vcp }) => {
                        tracing::info!(
                            monitor = monitor_id,
                            input = format!("0x{input_source_vcp:02x}"),
                            "received switch command"
                        );
                        let result = softkvm_core::ddc::switch_with_retry(
                            ddc.as_ref(),
                            &monitor_id,
                            input_source_vcp,
                            config.ddc.skip_if_current,
                            config.ddc.retry_count,
                            config.ddc.retry_delay_ms,
                        );
                        let (success, error) = match result {
                            Ok(_) => (true, None),
                            Err(e) => (false, Some(e.to_string())),
                        };
                        let _ = conn.send(&Message::SwitchAck {
                            monitor_id,
                            success,
                            error,
                        }).await;
                    }
                    Ok(Message::RequestInventory) => {
                        let monitors = ddc.enumerate_monitors().unwrap_or_default();
                        let _ = conn.send(&Message::MonitorInventory { monitors }).await;
                    }
                    Ok(Message::Heartbeat { .. }) => {
                        // heartbeat echo from server, no action needed
                    }
                    Ok(other) => {
                        tracing::debug!(msg = ?other, "unexpected message");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "connection lost, reconnecting");
                        conn = connection::connect_with_retry(addr, &agent_name, 3000, 30000).await;
                        // re-send inventory after reconnect
                        let monitors = ddc.enumerate_monitors().unwrap_or_default();
                        let _ = conn.send(&Message::MonitorInventory { monitors }).await;
                    }
                }
            }
            // send periodic heartbeats
            _ = heartbeat_interval.tick() => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if let Err(e) = conn.send(&Message::Heartbeat { timestamp_ms: ts }).await {
                    tracing::warn!(error = %e, "heartbeat send failed, reconnecting");
                    conn = connection::connect_with_retry(addr, &agent_name, 3000, 30000).await;
                }
            }
            // handle ctrl+c
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received shutdown signal");
                break;
            }
        }
    }

    tracing::info!("agent shut down");
    Ok(())
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown-agent".into())
}

fn create_controller() -> Box<dyn DdcController> {
    softkvm_core::ddc::create_controller()
}
