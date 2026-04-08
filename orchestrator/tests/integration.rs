use std::sync::Arc;

use full_kvm_core::ddc::DdcController;
use full_kvm_core::protocol::{
    self, DaemonState, JsonRpcRequest, JsonRpcResponse, MachineState, Message, MonitorInfo,
    PROTOCOL_VERSION,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tokio::sync::{mpsc, RwLock};
use tokio::time::Duration;

/// end-to-end: agent connects to orchestrator listener, performs handshake,
/// sends inventory, receives switch command, sends ack
#[tokio::test]
async fn test_full_switch_flow() {
    // set up orchestrator agent listener on ephemeral port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (event_tx, mut event_rx) = mpsc::channel(32);

    // accept one agent connection manually (simulating the agent listener)
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        // receive AgentHello
        let msg = protocol::read_message(&mut reader).await.unwrap();
        let agent_name = match msg {
            Message::AgentHello { agent_name, version } => {
                assert_eq!(version, PROTOCOL_VERSION);
                event_tx
                    .send(format!("connected:{agent_name}"))
                    .await
                    .unwrap();
                agent_name
            }
            other => panic!("expected AgentHello, got {:?}", other),
        };

        // send OrchestratorHello
        protocol::write_message(
            &mut writer,
            &Message::OrchestratorHello {
                version: PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();

        // receive MonitorInventory
        let msg = protocol::read_message(&mut reader).await.unwrap();
        match msg {
            Message::MonitorInventory { monitors } => {
                assert_eq!(monitors.len(), 1);
                event_tx
                    .send(format!("inventory:{}", monitors[0].id))
                    .await
                    .unwrap();
            }
            other => panic!("expected MonitorInventory, got {:?}", other),
        }

        // send SwitchMonitor command
        protocol::write_message(
            &mut writer,
            &Message::SwitchMonitor {
                monitor_id: "TST:MON:001".into(),
                input_source_vcp: 0x11,
            },
        )
        .await
        .unwrap();

        // receive SwitchAck
        let msg = protocol::read_message(&mut reader).await.unwrap();
        match msg {
            Message::SwitchAck {
                monitor_id,
                success,
                ..
            } => {
                assert_eq!(monitor_id, "TST:MON:001");
                assert!(success);
                event_tx.send("ack:success".into()).await.unwrap();
            }
            other => panic!("expected SwitchAck, got {:?}", other),
        }

        agent_name
    });

    // simulate agent side
    let agent = tokio::spawn(async move {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        // send AgentHello
        protocol::write_message(
            &mut writer,
            &Message::AgentHello {
                agent_name: "MacBook".into(),
                version: PROTOCOL_VERSION,
            },
        )
        .await
        .unwrap();

        // receive OrchestratorHello
        let msg = protocol::read_message(&mut reader).await.unwrap();
        assert!(matches!(msg, Message::OrchestratorHello { .. }));

        // send MonitorInventory
        protocol::write_message(
            &mut writer,
            &Message::MonitorInventory {
                monitors: vec![MonitorInfo {
                    id: "TST:MON:001".into(),
                    name: "Test Monitor".into(),
                    manufacturer: "TST".into(),
                    model: "MON".into(),
                    serial: "001".into(),
                    current_input_vcp: Some(0x0f),
                    ddc_supported: true,
                }],
            },
        )
        .await
        .unwrap();

        // receive SwitchMonitor
        let msg = protocol::read_message(&mut reader).await.unwrap();
        match msg {
            Message::SwitchMonitor {
                monitor_id,
                input_source_vcp,
            } => {
                assert_eq!(monitor_id, "TST:MON:001");
                assert_eq!(input_source_vcp, 0x11);
            }
            other => panic!("expected SwitchMonitor, got {:?}", other),
        }

        // send SwitchAck
        protocol::write_message(
            &mut writer,
            &Message::SwitchAck {
                monitor_id: "TST:MON:001".into(),
                success: true,
                error: None,
            },
        )
        .await
        .unwrap();
    });

    agent.await.unwrap();
    let agent_name = server.await.unwrap();
    assert_eq!(agent_name, "MacBook");

    // verify event sequence
    let mut events = Vec::new();
    while let Ok(evt) = event_rx.try_recv() {
        events.push(evt);
    }
    assert_eq!(events[0], "connected:MacBook");
    assert_eq!(events[1], "inventory:TST:MON:001");
    assert_eq!(events[2], "ack:success");
}

/// IPC end-to-end: start IPC server, connect, get_state, switch_machine
#[tokio::test]
async fn test_ipc_end_to_end() {
    let socket = format!("/tmp/full-kvm-integ-{}.sock", std::process::id());
    let _ = std::fs::remove_file(&socket);

    let (cmd_tx, mut cmd_rx) = mpsc::channel(32);
    let daemon_state = Arc::new(RwLock::new(DaemonState {
        machines: vec![
            MachineState {
                name: "Windows-PC".into(),
                os: "windows".into(),
                role: "server".into(),
                online: true,
                active: true,
            },
            MachineState {
                name: "MacBook".into(),
                os: "macos".into(),
                role: "client".into(),
                online: true,
                active: false,
            },
        ],
        monitors: vec![MonitorInfo {
            id: "DEL:U2720Q:SN123".into(),
            name: "Dell U2720Q".into(),
            manufacturer: "Dell".into(),
            model: "U2720Q".into(),
            serial: "SN123".into(),
            current_input_vcp: Some(0x0f),
            ddc_supported: true,
        }],
        active_machine: Some("Windows-PC".into()),
        focus_locked: false,
        deskflow_status: "running".into(),
    }));

    // start IPC server
    let sock = socket.clone();
    let state = daemon_state.clone();
    tokio::spawn(async move {
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let st = Arc::new(RwLock::new(state.read().await.clone()));
            let tx = cmd_tx.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let buf_reader = BufReader::new(reader);
                let mut lines = buf_reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let req: JsonRpcRequest = serde_json::from_str(&line).unwrap();
                    let resp = match req.method.as_str() {
                        "get_state" => {
                            let ds = st.read().await;
                            JsonRpcResponse::success(
                                req.id.clone(),
                                serde_json::to_value(&*ds).unwrap(),
                            )
                        }
                        "switch_machine" => {
                            let machine = req
                                .params
                                .as_ref()
                                .and_then(|p| p.get("machine"))
                                .and_then(|v| v.as_str())
                                .unwrap();
                            let _ = tx.send(machine.to_string()).await;
                            JsonRpcResponse::success(
                                req.id.clone(),
                                serde_json::json!({"status": "ok"}),
                            )
                        }
                        _ => JsonRpcResponse::error(req.id.clone(), -32601, "unknown".into()),
                    };
                    let mut json = serde_json::to_string(&resp).unwrap();
                    json.push('\n');
                    writer.write_all(json.as_bytes()).await.unwrap();
                    writer.flush().await.unwrap();
                }
            });
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // connect and make requests
    let mut stream = UnixStream::connect(&socket).await.unwrap();

    // get_state
    let req = r#"{"jsonrpc":"2.0","method":"get_state","id":1}"#;
    stream.write_all(format!("{req}\n").as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_slice(&buf[..n]).unwrap();
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["active_machine"], "Windows-PC");
    assert_eq!(result["machines"].as_array().unwrap().len(), 2);
    assert_eq!(result["monitors"].as_array().unwrap().len(), 1);

    // switch_machine
    let req = r#"{"jsonrpc":"2.0","method":"switch_machine","params":{"machine":"MacBook"},"id":2}"#;
    stream.write_all(format!("{req}\n").as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_slice(&buf[..n]).unwrap();
    assert!(resp.error.is_none());

    // verify the command was received
    let cmd = cmd_rx.try_recv().unwrap();
    assert_eq!(cmd, "MacBook");

    std::fs::remove_file(&socket).ok();
}

/// config -> topology -> switch pipeline test
#[tokio::test]
async fn test_config_topology_switch_pipeline() {
    use full_kvm_core::config::Config;
    use full_kvm_core::ddc::stub::StubDdcController;
    use std::collections::HashMap;

    let toml = r#"
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
name = "Dell U2720Q"
monitor_id = "TEST:MON:001"
connected_to = "Windows-PC"

[monitor.inputs]
"Windows-PC" = "DisplayPort1"
"MacBook" = "HDMI1"

[layout]
"Windows-PC" = { right = "MacBook" }
"MacBook" = { left = "Windows-PC" }
"#;

    let config = Config::from_toml(toml).unwrap();

    let topology = config.topology();
    let server = topology.server().unwrap();
    assert_eq!(server.name, "Windows-PC");

    // verify monitor is found for a transition to MacBook
    let transition_monitors = topology.monitors_for_transition("MacBook");
    assert_eq!(transition_monitors.len(), 1);
    assert_eq!(transition_monitors[0].0.monitor_id, "TEST:MON:001");

    // resolve the target input
    let target_input = transition_monitors[0].1;
    let aliases: HashMap<String, u16> = HashMap::new();
    let source = full_kvm_core::input_source::InputSource::from_str_with_aliases(target_input, &aliases).unwrap();
    let vcp = source.to_vcp_value(&aliases);
    assert_eq!(vcp, 0x11); // HDMI1

    // execute switch on stub controller
    let controller = StubDdcController::new();
    let switched = full_kvm_core::ddc::switch_with_retry(
        &controller,
        "TEST:MON:001",
        vcp,
        false,
        3,
        10,
    )
    .unwrap();
    assert!(switched);
    assert_eq!(controller.get_input_source("TEST:MON:001").unwrap(), 0x11);
}

/// protocol codec: multi-message stream integrity
#[tokio::test]
async fn test_protocol_stream_integrity() {
    let (mut client, mut server) = tokio::io::duplex(8192);

    // write a sequence of mixed messages
    let messages = vec![
        Message::AgentHello {
            agent_name: "test".into(),
            version: PROTOCOL_VERSION,
        },
        Message::OrchestratorHello {
            version: PROTOCOL_VERSION,
        },
        Message::MonitorInventory {
            monitors: vec![MonitorInfo {
                id: "A:B:C".into(),
                name: "Mon".into(),
                manufacturer: "A".into(),
                model: "B".into(),
                serial: "C".into(),
                current_input_vcp: Some(0x0f),
                ddc_supported: true,
            }],
        },
        Message::SwitchMonitor {
            monitor_id: "A:B:C".into(),
            input_source_vcp: 0x11,
        },
        Message::SwitchAck {
            monitor_id: "A:B:C".into(),
            success: true,
            error: None,
        },
        Message::Heartbeat {
            timestamp_ms: 123456789,
        },
        Message::RequestInventory,
    ];

    for msg in &messages {
        protocol::write_message(&mut client, msg).await.unwrap();
    }
    drop(client);

    // read them all back and verify
    for expected in &messages {
        let decoded = protocol::read_message(&mut server).await.unwrap();
        assert_eq!(
            serde_json::to_string(expected).unwrap(),
            serde_json::to_string(&decoded).unwrap(),
        );
    }

    // next read should fail (EOF)
    assert!(protocol::read_message(&mut server).await.is_err());
}
