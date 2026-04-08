use std::collections::HashMap;
use std::sync::Arc;

use softkvm_core::protocol::{self, Message, MonitorInfo, PROTOCOL_VERSION};
use tokio::io::BufReader;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{Duration, Instant};

/// metadata about a connected agent
#[derive(Debug)]
struct AgentInfo {
    name: String,
    #[allow(dead_code)]
    version: u16,
    monitors: Vec<MonitorInfo>,
    last_heartbeat: Instant,
    writer: tokio::io::WriteHalf<TcpStream>,
}

/// events the agent listener sends to the orchestrator main loop
#[derive(Debug)]
pub enum AgentEvent {
    Connected(String),
    Disconnected(String),
    MonitorInventory {
        agent_name: String,
        monitors: Vec<MonitorInfo>,
    },
    SwitchAck {
        agent_name: String,
        monitor_id: String,
        success: bool,
        error: Option<String>,
    },
}

/// handle for the orchestrator to interact with connected agents
#[derive(Clone)]
pub struct AgentManager {
    agents: Arc<RwLock<HashMap<String, Arc<Mutex<AgentInfo>>>>>,
    event_tx: mpsc::Sender<AgentEvent>,
}

impl AgentManager {
    pub fn new(event_tx: mpsc::Sender<AgentEvent>) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        }
    }

    /// send a SwitchMonitor command to the agent controlling the given monitor.
    /// called when a remote monitor needs switching during a screen transition
    #[allow(dead_code)]
    pub async fn send_switch(
        &self,
        agent_name: &str,
        monitor_id: &str,
        input_source_vcp: u16,
    ) -> anyhow::Result<()> {
        let agents = self.agents.read().await;
        let agent = agents
            .get(agent_name)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not connected", agent_name))?;

        let msg = Message::SwitchMonitor {
            monitor_id: monitor_id.into(),
            input_source_vcp,
        };
        let mut info = agent.lock().await;
        protocol::write_message(&mut info.writer, &msg).await?;
        Ok(())
    }

    /// request a monitor re-scan from an agent
    pub async fn request_inventory(&self, agent_name: &str) -> anyhow::Result<()> {
        let agents = self.agents.read().await;
        let agent = agents
            .get(agent_name)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not connected", agent_name))?;

        let mut info = agent.lock().await;
        protocol::write_message(&mut info.writer, &Message::RequestInventory).await?;
        Ok(())
    }

    /// list currently connected agent names
    pub async fn connected_agents(&self) -> Vec<String> {
        self.agents.read().await.keys().cloned().collect()
    }

    /// get monitors reported by a specific agent
    #[allow(dead_code)]
    pub async fn agent_monitors(&self, agent_name: &str) -> Vec<MonitorInfo> {
        let agents = self.agents.read().await;
        match agents.get(agent_name) {
            Some(agent) => agent.lock().await.monitors.clone(),
            None => vec![],
        }
    }
}

/// start listening for agent connections
pub async fn run_agent_listener(listen_addr: &str, manager: AgentManager) -> std::io::Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!(addr = listen_addr, "agent listener started");

    loop {
        let (stream, peer) = listener.accept().await?;
        tracing::info!(peer = %peer, "agent connection from");

        let mgr = manager.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_agent(stream, mgr).await {
                tracing::debug!(error = %e, "agent handler ended");
            }
        });
    }
}

async fn handle_agent(stream: TcpStream, manager: AgentManager) -> anyhow::Result<()> {
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    // expect AgentHello
    let hello = protocol::read_message(&mut reader).await?;
    let (agent_name, agent_version) = match hello {
        Message::AgentHello {
            agent_name,
            version,
        } => (agent_name, version),
        other => {
            anyhow::bail!("expected AgentHello, got {:?}", other);
        }
    };

    tracing::info!(
        agent = agent_name,
        version = agent_version,
        "agent handshake"
    );

    // respond with OrchestratorHello
    let agent_info = Arc::new(Mutex::new(AgentInfo {
        name: agent_name.clone(),
        version: agent_version,
        monitors: vec![],
        last_heartbeat: Instant::now(),
        writer,
    }));

    {
        let mut info = agent_info.lock().await;
        protocol::write_message(
            &mut info.writer,
            &Message::OrchestratorHello {
                version: PROTOCOL_VERSION,
            },
        )
        .await?;
    }

    // register agent
    manager
        .agents
        .write()
        .await
        .insert(agent_name.clone(), agent_info.clone());
    let _ = manager
        .event_tx
        .send(AgentEvent::Connected(agent_name.clone()))
        .await;

    // message loop
    let result = agent_message_loop(&mut reader, &agent_info, &manager).await;

    // cleanup on disconnect
    manager.agents.write().await.remove(&agent_name);
    let _ = manager
        .event_tx
        .send(AgentEvent::Disconnected(agent_name.clone()))
        .await;
    tracing::info!(agent = agent_name, "agent disconnected");

    result
}

async fn agent_message_loop(
    reader: &mut BufReader<tokio::io::ReadHalf<TcpStream>>,
    agent_info: &Arc<Mutex<AgentInfo>>,
    manager: &AgentManager,
) -> anyhow::Result<()> {
    loop {
        let msg = protocol::read_message(reader).await?;
        let mut info = agent_info.lock().await;

        match msg {
            Message::Heartbeat { .. } => {
                info.last_heartbeat = Instant::now();
                // echo heartbeat back
                protocol::write_message(
                    &mut info.writer,
                    &Message::Heartbeat {
                        timestamp_ms: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    },
                )
                .await?;
            }
            Message::MonitorInventory { monitors } => {
                tracing::info!(
                    agent = info.name,
                    count = monitors.len(),
                    "received monitor inventory"
                );
                let _ = manager
                    .event_tx
                    .send(AgentEvent::MonitorInventory {
                        agent_name: info.name.clone(),
                        monitors: monitors.clone(),
                    })
                    .await;
                info.monitors = monitors;
            }
            Message::SwitchAck {
                monitor_id,
                success,
                error,
            } => {
                let _ = manager
                    .event_tx
                    .send(AgentEvent::SwitchAck {
                        agent_name: info.name.clone(),
                        monitor_id,
                        success,
                        error,
                    })
                    .await;
            }
            other => {
                tracing::warn!(agent = info.name, msg = ?other, "unexpected message from agent");
            }
        }
    }
}

/// check for stale agents that haven't sent a heartbeat recently
pub async fn check_heartbeats(manager: &AgentManager, timeout: Duration) -> Vec<String> {
    let agents = manager.agents.read().await;
    let now = Instant::now();
    let mut stale = Vec::new();
    for (name, agent) in agents.iter() {
        let info = agent.lock().await;
        if now.duration_since(info.last_heartbeat) > timeout {
            stale.push(name.clone());
        }
    }
    stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use softkvm_core::protocol::{self, Message, MonitorInfo, PROTOCOL_VERSION};
    use tokio::net::TcpStream;

    /// helper: start listener on ephemeral port, return address + manager + event rx
    async fn setup() -> (String, AgentManager, mpsc::Receiver<AgentEvent>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        let (event_tx, event_rx) = mpsc::channel(32);
        let manager = AgentManager::new(event_tx);

        let mgr = manager.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let m = mgr.clone();
                tokio::spawn(async move {
                    let _ = handle_agent(stream, m).await;
                });
            }
        });

        (addr, manager, event_rx)
    }

    /// helper: connect as agent, perform handshake
    async fn connect_agent(
        addr: &str,
        name: &str,
    ) -> (
        BufReader<tokio::io::ReadHalf<TcpStream>>,
        tokio::io::WriteHalf<TcpStream>,
    ) {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        // send AgentHello
        protocol::write_message(
            &mut writer,
            &Message::AgentHello {
                agent_name: name.into(),
                version: PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();

        // receive OrchestratorHello
        let resp = protocol::read_message(&mut reader).await.unwrap();
        assert!(matches!(resp, Message::OrchestratorHello { .. }));

        (reader, writer)
    }

    #[tokio::test]
    async fn test_agent_listener_accept() {
        let (addr, manager, mut event_rx) = setup().await;
        let (_reader, _writer) = connect_agent(&addr, "test-agent").await;

        // should get Connected event
        let evt = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(evt, AgentEvent::Connected(ref n) if n == "test-agent"));

        // verify agent is tracked
        let agents = manager.connected_agents().await;
        assert!(agents.contains(&"test-agent".to_string()));
    }

    #[tokio::test]
    async fn test_agent_sends_monitor_inventory() {
        let (addr, manager, mut event_rx) = setup().await;
        let (_reader, mut writer) = connect_agent(&addr, "inv-agent").await;

        // consume Connected event
        let _ = event_rx.recv().await;

        // send inventory
        protocol::write_message(
            &mut writer,
            &Message::MonitorInventory {
                monitors: vec![MonitorInfo {
                    id: "TEST:MON:001".into(),
                    name: "Test".into(),
                    manufacturer: "TST".into(),
                    model: "MON".into(),
                    serial: "001".into(),
                    current_input_vcp: Some(0x11),
                    ddc_supported: true,
                }],
            },
        )
        .await
        .unwrap();

        // should get inventory event
        let evt = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match evt {
            AgentEvent::MonitorInventory {
                agent_name,
                monitors,
            } => {
                assert_eq!(agent_name, "inv-agent");
                assert_eq!(monitors.len(), 1);
                assert_eq!(monitors[0].id, "TEST:MON:001");
            }
            other => panic!("expected MonitorInventory, got {:?}", other),
        }

        // verify monitors stored on manager
        let mons = manager.agent_monitors("inv-agent").await;
        assert_eq!(mons.len(), 1);
    }

    #[tokio::test]
    async fn test_agent_handles_switch_command() {
        let (addr, manager, mut event_rx) = setup().await;
        let (mut reader, mut writer) = connect_agent(&addr, "switch-agent").await;

        // consume Connected event
        let _ = event_rx.recv().await;

        // send inventory first so the agent is registered with monitors
        protocol::write_message(
            &mut writer,
            &Message::MonitorInventory {
                monitors: vec![MonitorInfo {
                    id: "M1".into(),
                    name: "Mon".into(),
                    manufacturer: "X".into(),
                    model: "Y".into(),
                    serial: "Z".into(),
                    current_input_vcp: Some(0x0f),
                    ddc_supported: true,
                }],
            },
        )
        .await
        .unwrap();

        // wait for inventory event
        let _ = tokio::time::timeout(Duration::from_secs(1), event_rx.recv()).await;

        // send switch command from orchestrator
        manager
            .send_switch("switch-agent", "M1", 0x11)
            .await
            .unwrap();

        // agent should receive SwitchMonitor
        let msg = tokio::time::timeout(Duration::from_secs(1), protocol::read_message(&mut reader))
            .await
            .unwrap()
            .unwrap();
        match msg {
            Message::SwitchMonitor {
                monitor_id,
                input_source_vcp,
            } => {
                assert_eq!(monitor_id, "M1");
                assert_eq!(input_source_vcp, 0x11);
            }
            other => panic!("expected SwitchMonitor, got {:?}", other),
        }

        // agent sends SwitchAck back
        protocol::write_message(
            &mut writer,
            &Message::SwitchAck {
                monitor_id: "M1".into(),
                success: true,
                error: None,
            },
        )
        .await
        .unwrap();

        // should get SwitchAck event
        let evt = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match evt {
            AgentEvent::SwitchAck {
                agent_name,
                monitor_id,
                success,
                ..
            } => {
                assert_eq!(agent_name, "switch-agent");
                assert_eq!(monitor_id, "M1");
                assert!(success);
            }
            other => panic!("expected SwitchAck, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_agent_heartbeat() {
        let (addr, _manager, mut event_rx) = setup().await;
        let (mut reader, mut writer) = connect_agent(&addr, "hb-agent").await;

        // consume Connected event
        let _ = event_rx.recv().await;

        // send heartbeat
        protocol::write_message(
            &mut writer,
            &Message::Heartbeat {
                timestamp_ms: 12345,
            },
        )
        .await
        .unwrap();

        // should get heartbeat echo back
        let msg = tokio::time::timeout(Duration::from_secs(1), protocol::read_message(&mut reader))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(msg, Message::Heartbeat { .. }));
    }

    #[tokio::test]
    async fn test_agent_disconnect_event() {
        let (addr, _manager, mut event_rx) = setup().await;
        let (_reader, writer) = connect_agent(&addr, "disc-agent").await;

        // consume Connected event
        let _ = event_rx.recv().await;

        // drop the writer to disconnect
        drop(writer);
        drop(_reader);

        // should get Disconnected event
        let evt = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(evt, AgentEvent::Disconnected(ref n) if n == "disc-agent"));
    }
}
