use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use softkvm_core::config::Config;
use softkvm_core::ddc::DdcController;
use softkvm_core::protocol::{DaemonState, MachineState};
use tokio::sync::{mpsc, RwLock};
use tokio::time::Duration;

mod agent_listener;
mod deskflow;
mod discovery;
mod ipc_server;
mod key_interceptor;
mod log_parser;
mod switch_engine;

use agent_listener::{AgentEvent, AgentManager};
use ipc_server::{IpcCommand, IpcState};

#[derive(Parser)]
#[command(name = "softkvm-orchestrator", about = "softkvm orchestrator daemon")]
struct Cli {
    /// path to config file
    #[arg(short, long, default_value = "softkvm.toml")]
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
    let (config, config_path) =
        Config::load_or_find(&cli.config).map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::info!(config = config_path, "using config");

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

    // select DDC controller: real ddc-hi on supported platforms, logging stub otherwise
    let ddc: Box<dyn DdcController> = create_ddc_controller();
    let engine = switch_engine::SwitchEngine::new(config.clone());

    // build OS lookup for machines (name -> OsType)
    let machine_os: std::collections::HashMap<String, softkvm_core::keymap::OsType> = config
        .machines
        .iter()
        .map(|m| (m.name.clone(), m.os))
        .collect();

    // determine local OS from the server machine entry
    let local_os = machine_os
        .get(&server_name)
        .copied()
        .unwrap_or(softkvm_core::keymap::OsType::Linux);

    // -- shared daemon state for IPC clients --
    let daemon_state = Arc::new(RwLock::new(DaemonState {
        machines: config
            .machines
            .iter()
            .map(|m| MachineState {
                name: m.name.clone(),
                os: format!("{}", m.os),
                role: format!("{:?}", m.role).to_lowercase(),
                online: m.name == server_name, // only local machine is online at startup
                active: m.name == server_name,
            })
            .collect(),
        monitors: match ddc.enumerate_monitors() {
            Ok(monitors) => monitors,
            Err(e) => {
                tracing::warn!(error = %e, "failed to enumerate monitors at startup");
                vec![]
            }
        },
        active_machine: Some(server_name.clone()),
        focus_locked: false,
        deskflow_status: if config.deskflow.managed && !cli.no_deskflow {
            "starting".into()
        } else {
            "disabled".into()
        },
    }));

    // -- IPC server --
    let (ipc_cmd_tx, mut ipc_cmd_rx) = mpsc::channel::<IpcCommand>(32);
    let ipc_state = IpcState {
        daemon_state: daemon_state.clone(),
        cmd_tx: ipc_cmd_tx.clone(),
    };
    let ipc_socket = ipc_server::default_socket_path();
    let ipc_st = ipc_state.clone();
    tokio::spawn(async move {
        if let Err(e) = ipc_server::run_ipc_server(&ipc_socket, ipc_st).await {
            tracing::error!(error = %e, "IPC server failed");
        }
    });

    // -- agent listener for remote agent connections --
    let config_arc = Arc::new(config.clone());
    let (agent_event_tx, mut agent_event_rx) = mpsc::channel::<AgentEvent>(32);
    let agent_manager =
        AgentManager::new(agent_event_tx, daemon_state.clone(), config_arc, ipc_cmd_tx);
    let listen_addr = format!("0.0.0.0:{}", config.network.listen_port);
    let mgr = agent_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = agent_listener::run_agent_listener(&listen_addr, mgr).await {
            tracing::error!(error = %e, "agent listener failed");
        }
    });

    // -- heartbeat checker --
    let hb_manager = agent_manager.clone();
    let hb_state = daemon_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let stale =
                agent_listener::check_heartbeats(&hb_manager, Duration::from_secs(30)).await;
            if !stale.is_empty() {
                tracing::warn!(agents = ?stale, "stale agent connections detected");
            }
            // update online status in daemon state
            let connected = hb_manager.connected_agents().await;
            let mut ds = hb_state.write().await;
            let local = ds.active_machine.clone().unwrap_or_default();
            for machine in &mut ds.machines {
                if machine.name == local {
                    continue; // local machine is always online
                }
                machine.online = connected.contains(&machine.name);
            }
        }
    });

    // -- discovery responder --
    let disc_name = server_name.clone();
    let disc_port = config.network.listen_port;
    let disc_os = local_os.to_string();
    tokio::spawn(async move {
        if let Err(e) = discovery::run_discovery_responder(
            disc_name,
            env!("CARGO_PKG_VERSION").to_string(),
            disc_port,
            disc_os,
        )
        .await
        {
            tracing::warn!(error = %e, "discovery responder failed");
        }
    });

    // -- key interceptor --
    let mut interceptor = key_interceptor::KeyInterceptor::new(
        local_os,
        config.keyboard.translations.clone(),
        config.keyboard.auto_remap,
    );
    if let Err(e) = interceptor.start() {
        tracing::warn!(error = %e, "key interceptor unavailable, continuing without combo translation");
    }

    // -- deskflow process --
    let (log_tx, mut log_rx) = mpsc::channel::<String>(256);
    let mut deskflow_process: Option<deskflow::DeskflowProcess> = None;
    if config.deskflow.managed && !cli.no_deskflow {
        match deskflow::DeskflowProcess::spawn(&config).await {
            Ok((process, mut deskflow_rx)) => {
                deskflow_process = Some(process);
                let tx = log_tx.clone();
                tokio::spawn(async move {
                    while let Some(line) = deskflow_rx.recv().await {
                        if tx.send(line).await.is_err() {
                            break;
                        }
                    }
                });
                daemon_state.write().await.deskflow_status = "running".into();
                tracing::info!("deskflow-core server started");
            }
            Err(e) => {
                tracing::warn!(error = %e, "deskflow-core not available, running without cursor-edge switching");
                tracing::warn!("install deskflow and ensure deskflow-core is in PATH to enable it");
                daemon_state.write().await.deskflow_status = format!("error: {e}");
            }
        }
    } else {
        tracing::info!("deskflow management disabled, running without deskflow");
    }
    drop(log_tx);

    let mut deskflow_active = deskflow_process.is_some();

    tracing::info!("orchestrator running");

    // -- main event loop --
    loop {
        tokio::select! {
            // deskflow log lines -> transition detection
            line = log_rx.recv(), if deskflow_active => {
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

                            let remote_os = if event.target == server_name {
                                None
                            } else {
                                machine_os.get(&event.target).copied()
                            };
                            interceptor.set_remote_os(remote_os);

                            // update active machine in daemon state
                            {
                                let mut ds = daemon_state.write().await;
                                ds.active_machine = Some(event.target.clone());
                                for machine in &mut ds.machines {
                                    machine.active = machine.name == event.target;
                                }
                            }

                            let results = engine.handle_transition(&event, ddc.as_ref());
                            dispatch_switch_results(&results, &agent_manager).await;
                        }
                    }
                    None => {
                        tracing::warn!("deskflow log channel closed");
                        daemon_state.write().await.deskflow_status = "stopped".into();
                        deskflow_active = false;
                    }
                }
            }

            // key interceptor events
            key_event = interceptor.recv() => {
                match key_event {
                    Some(key_interceptor::KeyEvent::Translated { intent, from, to }) => {
                        tracing::debug!(
                            intent = intent,
                            from = %format!("{from:?}"),
                            to = %format!("{to:?}"),
                            "key combo translated"
                        );
                    }
                    Some(key_interceptor::KeyEvent::Started) => {
                        tracing::info!("key interceptor hook active");
                    }
                    Some(key_interceptor::KeyEvent::Stopped) => {
                        tracing::info!("key interceptor hook stopped");
                    }
                    None => {
                        tracing::debug!("key interceptor channel closed");
                    }
                }
            }

            // IPC commands from control panel / CLI
            cmd = ipc_cmd_rx.recv() => {
                match cmd {
                    Some(IpcCommand::SwitchMachine(target)) => {
                        tracing::info!(target = target, "IPC: switch machine requested");
                        // synthesize a transition event as if deskflow triggered it
                        let current = daemon_state.read().await.active_machine.clone()
                            .unwrap_or_default();
                        let event = log_parser::TransitionEvent {
                            source: current,
                            target: target.clone(),
                            x: 0,
                            y: 0,
                        };

                        let remote_os = if target == server_name {
                            None
                        } else {
                            machine_os.get(&target).copied()
                        };
                        interceptor.set_remote_os(remote_os);

                        {
                            let mut ds = daemon_state.write().await;
                            ds.active_machine = Some(target.clone());
                            for machine in &mut ds.machines {
                                machine.active = machine.name == target;
                            }
                        }

                        let results = engine.handle_transition(&event, ddc.as_ref());
                        dispatch_switch_results(&results, &agent_manager).await;
                    }
                    Some(IpcCommand::TestSwitch { monitor_id, input }) => {
                        tracing::info!(monitor = monitor_id, input = input, "IPC: test switch");
                        let source = softkvm_core::input_source::InputSource::from_str_with_aliases(
                            &input,
                            &config.input_aliases,
                        );
                        if let Some(source) = source {
                            let vcp = source.to_vcp_value(&config.input_aliases);
                            match softkvm_core::ddc::switch_with_retry(
                                ddc.as_ref(),
                                &monitor_id,
                                vcp,
                                false,
                                config.ddc.retry_count,
                                config.ddc.retry_delay_ms,
                            ) {
                                Ok(_) => tracing::info!(monitor = monitor_id, "test switch succeeded"),
                                Err(e) => tracing::error!(monitor = monitor_id, error = %e, "test switch failed"),
                            }
                        } else {
                            tracing::error!(input = input, "unknown input source for test switch");
                        }
                    }
                    Some(IpcCommand::SetFocusLock(locked)) => {
                        tracing::info!(locked = locked, "IPC: focus lock");
                        interceptor.set_enabled(!locked);
                        daemon_state.write().await.focus_locked = locked;
                    }
                    Some(IpcCommand::RescanMonitors) => {
                        tracing::info!("IPC: rescan monitors");
                        match ddc.enumerate_monitors() {
                            Ok(monitors) => {
                                tracing::info!(count = monitors.len(), "monitor rescan complete");
                                daemon_state.write().await.monitors = monitors;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "monitor rescan failed");
                            }
                        }
                        // also ask all connected agents to rescan
                        for agent in agent_manager.connected_agents().await {
                            if let Err(e) = agent_manager.request_inventory(&agent).await {
                                tracing::warn!(agent = agent, error = %e, "failed to request agent rescan");
                            }
                        }
                    }
                    Some(IpcCommand::PushUpdate { dev }) => {
                        tracing::info!(dev = dev, "IPC: pushing update to agents");
                        for agent in agent_manager.connected_agents().await {
                            if let Err(e) = agent_manager.send_update(&agent, dev).await {
                                tracing::warn!(agent = agent, error = %e, "failed to push update to agent");
                            } else {
                                tracing::info!(agent = agent, "update pushed");
                            }
                        }
                    }
                    Some(IpcCommand::SetupTestSwitch { monitor_id, input_vcp, reply }) => {
                        tracing::info!(monitor = monitor_id, vcp = input_vcp, "setup: test switch");
                        let success = match softkvm_core::ddc::switch_with_retry(
                            ddc.as_ref(),
                            &monitor_id,
                            input_vcp,
                            false,
                            config.ddc.retry_count,
                            config.ddc.retry_delay_ms,
                        ) {
                            Ok(_) => true,
                            Err(e) => {
                                tracing::warn!(monitor = monitor_id, error = %e, "setup test switch failed");
                                false
                            }
                        };
                        let _ = reply.send(success);
                    }
                    None => {
                        tracing::debug!("IPC command channel closed");
                    }
                }
            }

            // agent events (connections, disconnections, inventory, switch acks)
            agent_evt = agent_event_rx.recv() => {
                match agent_evt {
                    Some(AgentEvent::Connected(name)) => {
                        tracing::info!(agent = name, "agent connected");
                        let mut ds = daemon_state.write().await;
                        for machine in &mut ds.machines {
                            if machine.name == name {
                                machine.online = true;
                            }
                        }
                    }
                    Some(AgentEvent::Disconnected(name)) => {
                        tracing::info!(agent = name, "agent disconnected");
                        let mut ds = daemon_state.write().await;
                        for machine in &mut ds.machines {
                            if machine.name == name {
                                machine.online = false;
                            }
                        }
                    }
                    Some(AgentEvent::MonitorInventory { agent_name, monitors }) => {
                        tracing::info!(
                            agent = agent_name,
                            count = monitors.len(),
                            "received monitor inventory from agent"
                        );
                        // merge remote monitors into daemon state
                        // TODO: deduplicate and tag monitors with their source agent
                        let mut ds = daemon_state.write().await;
                        ds.monitors.retain(|m| {
                            !monitors.iter().any(|rm| rm.id == m.id)
                        });
                        ds.monitors.extend(monitors);
                    }
                    Some(AgentEvent::SwitchAck { agent_name, monitor_id, success, error }) => {
                        if success {
                            tracing::info!(
                                agent = agent_name,
                                monitor = monitor_id,
                                "remote switch confirmed"
                            );
                        } else {
                            tracing::error!(
                                agent = agent_name,
                                monitor = monitor_id,
                                error = error.as_deref().unwrap_or("unknown"),
                                "remote switch failed"
                            );
                        }
                    }
                    None => {
                        tracing::debug!("agent event channel closed");
                    }
                }
            }

            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received shutdown signal");
                break;
            }
        }
    }

    // cleanup
    if let Some(mut proc) = deskflow_process {
        tracing::info!("stopping deskflow-core");
        let _ = proc.kill().await;
    }

    tracing::info!("orchestrator shut down");
    Ok(())
}

/// dispatch switch results: execute remote switches via agent, log outcomes
async fn dispatch_switch_results(
    results: &[switch_engine::SwitchResult],
    agent_manager: &AgentManager,
) {
    for r in results {
        if !r.local {
            if let (Some(ref agent_name), Some(vcp)) = (&r.connected_to, r.vcp) {
                tracing::info!(
                    monitor = r.monitor_id,
                    agent = agent_name.as_str(),
                    input = format!("0x{vcp:02x}"),
                    "dispatching remote DDC switch"
                );
                if let Err(e) = agent_manager
                    .send_switch(agent_name, &r.monitor_id, vcp)
                    .await
                {
                    tracing::error!(
                        monitor = r.monitor_id,
                        agent = agent_name.as_str(),
                        error = %e,
                        "failed to dispatch remote switch"
                    );
                }
            } else {
                tracing::warn!(
                    monitor = r.monitor_id,
                    "remote switch needed but no agent info available"
                );
            }
        } else if r.success {
            tracing::info!(monitor = r.monitor_id, "monitor switched");
        } else if let Some(ref err) = r.error {
            tracing::error!(monitor = r.monitor_id, error = err, "monitor switch failed");
        }
    }
}

/// select the appropriate DDC controller based on build features
fn create_ddc_controller() -> Box<dyn DdcController> {
    #[cfg(feature = "real-ddc")]
    {
        tracing::info!("using real DDC/CI controller");
        softkvm_core::ddc::create_controller()
    }

    #[cfg(not(feature = "real-ddc"))]
    {
        tracing::warn!(
            "built without real-ddc feature, DDC/CI commands will be logged but not executed. \
             rebuild with --features real-ddc for actual monitor control"
        );
        Box::new(LoggingStubController)
    }
}

/// DDC controller that logs commands but does not execute them.
/// used when the real-ddc feature is not enabled (e.g. development on WSL/Linux)
#[cfg(not(feature = "real-ddc"))]
struct LoggingStubController;

#[cfg(not(feature = "real-ddc"))]
impl DdcController for LoggingStubController {
    fn enumerate_monitors(
        &self,
    ) -> softkvm_core::error::Result<Vec<softkvm_core::protocol::MonitorInfo>> {
        tracing::debug!("no-op: enumerate_monitors (real-ddc feature not enabled)");
        Ok(vec![])
    }

    fn get_input_source(&self, monitor_id: &str) -> softkvm_core::error::Result<u16> {
        tracing::debug!(
            monitor = monitor_id,
            "no-op: get_input_source (real-ddc feature not enabled)"
        );
        Err(softkvm_core::error::SoftKvmError::Ddc(
            "real-ddc feature not enabled".into(),
        ))
    }

    fn set_input_source(&self, monitor_id: &str, value: u16) -> softkvm_core::error::Result<()> {
        tracing::info!(
            monitor = monitor_id,
            input = format!("0x{:02x}", value),
            "no-op: would switch monitor input (real-ddc feature not enabled)"
        );
        Ok(())
    }

    fn get_vcp_feature(&self, monitor_id: &str, code: u8) -> softkvm_core::error::Result<u16> {
        tracing::debug!(
            monitor = monitor_id,
            code = format!("0x{:02x}", code),
            "no-op: get_vcp_feature (real-ddc feature not enabled)"
        );
        Err(softkvm_core::error::SoftKvmError::Ddc(
            "real-ddc feature not enabled".into(),
        ))
    }

    fn set_vcp_feature(
        &self,
        monitor_id: &str,
        code: u8,
        value: u16,
    ) -> softkvm_core::error::Result<()> {
        tracing::info!(
            monitor = monitor_id,
            code = format!("0x{:02x}", code),
            value = value,
            "no-op: would set VCP feature (real-ddc feature not enabled)"
        );
        Ok(())
    }
}
