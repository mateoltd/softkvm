use std::net::SocketAddr;

use full_kvm_core::protocol::{self, Message, PROTOCOL_VERSION};
use tokio::io::BufReader;
use tokio::net::TcpStream;

/// active connection to the orchestrator
pub struct OrchestratorConnection {
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: tokio::io::WriteHalf<TcpStream>,
}

impl OrchestratorConnection {
    /// connect to the orchestrator and perform the handshake
    pub async fn connect(addr: SocketAddr, agent_name: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        // send AgentHello
        protocol::write_message(
            &mut writer,
            &Message::AgentHello {
                agent_name: agent_name.into(),
                version: PROTOCOL_VERSION,
            },
        )
        .await?;

        // expect OrchestratorHello
        let response = protocol::read_message(&mut reader).await?;
        match response {
            Message::OrchestratorHello { version } => {
                tracing::info!(
                    server_version = version,
                    "connected to orchestrator"
                );
            }
            other => {
                anyhow::bail!("expected OrchestratorHello, got {:?}", other);
            }
        }

        Ok(Self { reader, writer })
    }

    /// send a message to the orchestrator
    pub async fn send(&mut self, msg: &Message) -> anyhow::Result<()> {
        protocol::write_message(&mut self.writer, msg).await?;
        Ok(())
    }

    /// receive a message from the orchestrator
    pub async fn recv(&mut self) -> anyhow::Result<Message> {
        let msg = protocol::read_message(&mut self.reader).await?;
        Ok(msg)
    }
}

/// connect with exponential backoff retry
pub async fn connect_with_retry(
    addr: SocketAddr,
    agent_name: &str,
    base_delay_ms: u64,
    max_delay_ms: u64,
) -> OrchestratorConnection {
    let mut delay = base_delay_ms;
    loop {
        match OrchestratorConnection::connect(addr, agent_name).await {
            Ok(conn) => return conn,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    retry_ms = delay,
                    "failed to connect to orchestrator, retrying"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                delay = (delay * 2).min(max_delay_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use full_kvm_core::protocol::{self, Message, MonitorInfo, PROTOCOL_VERSION};
    use tokio::net::TcpListener;

    /// mock orchestrator server
    async fn mock_server(listener: TcpListener) -> (
        BufReader<tokio::io::ReadHalf<TcpStream>>,
        tokio::io::WriteHalf<TcpStream>,
    ) {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        // expect AgentHello
        let msg = protocol::read_message(&mut reader).await.unwrap();
        assert!(matches!(msg, Message::AgentHello { .. }));

        // send OrchestratorHello
        protocol::write_message(
            &mut writer,
            &Message::OrchestratorHello { version: PROTOCOL_VERSION },
        )
        .await
        .unwrap();

        (reader, writer)
    }

    #[tokio::test]
    async fn test_agent_hello_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(mock_server(listener));
        let conn = OrchestratorConnection::connect(addr, "test-agent").await.unwrap();

        let (_reader, _writer) = server.await.unwrap();
        drop(conn);
    }

    #[tokio::test]
    async fn test_agent_sends_monitor_inventory() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut reader, _writer) = mock_server(listener).await;
            // expect MonitorInventory
            let msg = protocol::read_message(&mut reader).await.unwrap();
            match msg {
                Message::MonitorInventory { monitors } => {
                    assert_eq!(monitors.len(), 1);
                    assert_eq!(monitors[0].id, "TST:MON:001");
                }
                other => panic!("expected MonitorInventory, got {:?}", other),
            }
        });

        let mut conn = OrchestratorConnection::connect(addr, "inv-agent").await.unwrap();
        conn.send(&Message::MonitorInventory {
            monitors: vec![MonitorInfo {
                id: "TST:MON:001".into(),
                name: "Test".into(),
                manufacturer: "TST".into(),
                model: "MON".into(),
                serial: "001".into(),
                current_input_vcp: Some(0x0f),
                ddc_supported: true,
            }],
        })
        .await
        .unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_agent_handles_switch_command() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (_reader, mut writer) = mock_server(listener).await;
            // send SwitchMonitor
            protocol::write_message(
                &mut writer,
                &Message::SwitchMonitor {
                    monitor_id: "M1".into(),
                    input_source_vcp: 0x11,
                },
            )
            .await
            .unwrap();
            (_reader, writer)
        });

        let mut conn = OrchestratorConnection::connect(addr, "switch-agent").await.unwrap();
        let msg = conn.recv().await.unwrap();
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

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_agent_heartbeat() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut reader, _writer) = mock_server(listener).await;
            let msg = protocol::read_message(&mut reader).await.unwrap();
            assert!(matches!(msg, Message::Heartbeat { .. }));
        });

        let mut conn = OrchestratorConnection::connect(addr, "hb-agent").await.unwrap();
        conn.send(&Message::Heartbeat { timestamp_ms: 99999 }).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_agent_reconnect_on_disconnect() {
        let listener1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener1.local_addr().unwrap();

        // first server: accept then drop immediately
        let server1 = tokio::spawn(async move {
            let (stream, _) = listener1.accept().await.unwrap();
            drop(stream);
        });

        // try to connect -- will fail handshake because server drops connection
        let result = OrchestratorConnection::connect(addr, "retry-agent").await;
        assert!(result.is_err(), "should fail when server drops connection");
        server1.await.unwrap();

        // start a new server on the same port, agent should be able to connect
        let listener2 = TcpListener::bind(addr).await.unwrap();
        let server2 = tokio::spawn(mock_server(listener2));

        let conn = OrchestratorConnection::connect(addr, "retry-agent").await;
        assert!(conn.is_ok(), "should succeed on second attempt");

        server2.await.unwrap();
    }
}
