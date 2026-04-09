use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Result, SoftKvmError};

/// Message types for the orchestrator <-> agent wire protocol.
/// Wire format: [4-byte big-endian length][1-byte type][payload]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Orchestrator -> Agent: switch a monitor to a specific input.
    SwitchMonitor {
        monitor_id: String,
        input_source_vcp: u16,
    },

    /// Agent -> Orchestrator: result of a switch attempt.
    SwitchAck {
        monitor_id: String,
        success: bool,
        error: Option<String>,
    },

    /// Bidirectional: keepalive.
    Heartbeat { timestamp_ms: u64 },

    /// Agent -> Orchestrator: report connected monitors.
    MonitorInventory { monitors: Vec<MonitorInfo> },

    /// Orchestrator -> Agent: ask agent to re-scan monitors.
    RequestInventory,

    /// Agent -> Orchestrator: initial identification.
    AgentHello { agent_name: String, version: u16 },

    /// Orchestrator -> Agent: accept connection.
    OrchestratorHello { version: u16 },

    /// Agent -> Orchestrator: running applications list.
    AppList { apps: Vec<AppInfo> },

    /// Orchestrator -> Agent: request agent to self-update.
    RequestUpdate { dev: bool },

    /// Agent -> Orchestrator: result of self-update.
    UpdateAck {
        success: bool,
        new_version: Option<String>,
        error: Option<String>,
    },

    /// Setup wizard -> Orchestrator: request server state for assisted setup
    SetupQuery,

    /// Orchestrator -> Setup wizard: server state for assisted setup
    SetupInfo {
        server_name: String,
        os: String,
        monitors: Vec<MonitorInfo>,
        monitor_inputs: Vec<SetupMonitorMapping>,
    },

    /// Setup wizard -> Orchestrator: ask server to switch a monitor input
    SetupTestSwitch {
        monitor_id: String,
        input_vcp: u16,
    },

    /// Orchestrator -> Setup wizard: result of a test switch
    SetupTestSwitchAck {
        monitor_id: String,
        input_vcp: u16,
        success: bool,
    },
}

/// Information about a monitor discovered via DDC/CI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
    pub serial: String,
    pub current_input_vcp: Option<u16>,
    pub ddc_supported: bool,
}

/// Information about a running application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub title: String,
    pub pid: u32,
    pub icon_base64: Option<String>,
}

/// monitor input mapping from server config, used during assisted setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupMonitorMapping {
    pub monitor_id: String,
    pub inputs: std::collections::HashMap<String, String>,
}

/// Protocol version. Bump when wire format changes.
pub const PROTOCOL_VERSION: u16 = 1;

/// Discovery protocol constants.
pub const DISCOVERY_PORT: u16 = 24802;
pub const DISCOVERY_MAGIC: &[u8] = b"SOFTKVM_DISCOVER";
pub const DISCOVERY_RESPONSE_PREFIX: &str = "SOFTKVM_HERE";

/// Format a discovery response.
pub fn discovery_response(server_name: &str, version: &str, ip: &str, port: u16, os: &str) -> String {
    format!("{DISCOVERY_RESPONSE_PREFIX}:{server_name}:{version}:{ip}:{port}:{os}")
}

/// Parse a discovery response.
pub fn parse_discovery_response(msg: &str) -> Option<DiscoveryInfo> {
    let parts: Vec<&str> = msg.splitn(7, ':').collect();
    if parts.len() != 6 || parts[0] != DISCOVERY_RESPONSE_PREFIX {
        return None;
    }
    Some(DiscoveryInfo {
        server_name: parts[1].to_string(),
        version: parts[2].to_string(),
        ip: parts[3].to_string(),
        port: parts[4].parse().ok()?,
        os: parts[5].to_string(),
    })
}

#[derive(Debug, Clone)]
pub struct DiscoveryInfo {
    pub server_name: String,
    pub version: String,
    pub ip: String,
    pub port: u16,
    pub os: String,
}

// --- JSON-RPC types for IPC (orchestrator <-> Electron UI) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(JsonRpcError { code, message }),
            id,
        }
    }
}

/// snapshot of daemon state exposed to the UI via IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonState {
    pub machines: Vec<MachineState>,
    pub monitors: Vec<MonitorInfo>,
    pub active_machine: Option<String>,
    pub focus_locked: bool,
    pub deskflow_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineState {
    pub name: String,
    pub os: String,
    pub role: String,
    pub online: bool,
    pub active: bool,
}

// --- Wire Protocol Codec ---
// Frame: [4-byte BE length of (1 + json.len())][1-byte type][JSON payload]
// The type byte enables future binary routing without parsing JSON.
// The JSON payload carries the full serde-tagged Message enum.

/// Maximum message payload size (16 MB) to prevent OOM on malformed frames
const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

/// Map a Message variant to its type byte
pub fn message_type_byte(msg: &Message) -> u8 {
    match msg {
        Message::SwitchMonitor { .. } => 0x01,
        Message::SwitchAck { .. } => 0x02,
        Message::Heartbeat { .. } => 0x03,
        Message::MonitorInventory { .. } => 0x04,
        Message::RequestInventory => 0x05,
        Message::AgentHello { .. } => 0x06,
        Message::OrchestratorHello { .. } => 0x07,
        Message::AppList { .. } => 0x08,
        Message::RequestUpdate { .. } => 0x09,
        Message::UpdateAck { .. } => 0x0A,
        Message::SetupQuery => 0x0B,
        Message::SetupInfo { .. } => 0x0C,
        Message::SetupTestSwitch { .. } => 0x0D,
        Message::SetupTestSwitchAck { .. } => 0x0E,
    }
}

/// Encode a Message into a framed byte buffer
pub fn encode_message(msg: &Message) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(msg)?;
    let payload_len = 1 + json.len(); // type byte + json
    let mut buf = Vec::with_capacity(4 + payload_len);
    buf.extend_from_slice(&(payload_len as u32).to_be_bytes());
    buf.push(message_type_byte(msg));
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Decode a framed byte buffer into a Message.
/// `frame` should include the type byte + JSON payload (NOT the 4-byte length header).
pub fn decode_message(frame: &[u8]) -> Result<Message> {
    if frame.is_empty() {
        return Err(SoftKvmError::Protocol("empty frame".into()));
    }
    // skip the type byte (informational), deserialize from JSON
    let json_payload = &frame[1..];
    let msg: Message = serde_json::from_slice(json_payload)?;
    Ok(msg)
}

/// Read a single framed message from an async reader
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let payload_len = u32::from_be_bytes(len_buf);

    if payload_len == 0 {
        return Err(SoftKvmError::Protocol("zero-length frame".into()));
    }
    if payload_len > MAX_FRAME_SIZE {
        return Err(SoftKvmError::Protocol(format!(
            "frame too large: {payload_len} bytes (max {MAX_FRAME_SIZE})"
        )));
    }

    let mut payload = vec![0u8; payload_len as usize];
    reader.read_exact(&mut payload).await?;
    decode_message(&payload)
}

/// Write a single framed message to an async writer
pub async fn write_message<W: AsyncWrite + Unpin>(writer: &mut W, msg: &Message) -> Result<()> {
    let encoded = encode_message(msg)?;
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_roundtrip() {
        let resp = discovery_response("Windows-PC", "0.1.0", "192.168.1.100", 24801, "windows");
        let info = parse_discovery_response(&resp).unwrap();
        assert_eq!(info.server_name, "Windows-PC");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.ip, "192.168.1.100");
        assert_eq!(info.port, 24801);
        assert_eq!(info.os, "windows");
    }

    #[test]
    fn test_discovery_invalid() {
        assert!(parse_discovery_response("garbage").is_none());
        assert!(parse_discovery_response("SOFTKVM_HERE:a:b").is_none());
        assert!(parse_discovery_response("SOFTKVM_HERE:a:b:c:d").is_none());
    }

    // --- codec tests ---

    #[test]
    fn test_message_type_bytes_distinct() {
        let messages: Vec<Message> = vec![
            Message::SwitchMonitor {
                monitor_id: "m".into(),
                input_source_vcp: 0x11,
            },
            Message::SwitchAck {
                monitor_id: "m".into(),
                success: true,
                error: None,
            },
            Message::Heartbeat { timestamp_ms: 0 },
            Message::MonitorInventory { monitors: vec![] },
            Message::RequestInventory,
            Message::AgentHello {
                agent_name: "a".into(),
                version: 1,
            },
            Message::OrchestratorHello { version: 1 },
            Message::AppList { apps: vec![] },
            Message::RequestUpdate { dev: false },
            Message::UpdateAck {
                success: true,
                new_version: None,
                error: None,
            },
            Message::SetupQuery,
            Message::SetupInfo {
                server_name: "s".into(),
                os: "windows".into(),
                monitors: vec![],
                monitor_inputs: vec![],
            },
            Message::SetupTestSwitch {
                monitor_id: "m".into(),
                input_vcp: 0x11,
            },
            Message::SetupTestSwitchAck {
                monitor_id: "m".into(),
                input_vcp: 0x11,
                success: true,
            },
        ];
        let bytes: Vec<u8> = messages.iter().map(message_type_byte).collect();
        let mut unique = bytes.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(bytes.len(), unique.len(), "type bytes must be unique");
        assert_eq!(
            bytes,
            vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E]
        );
    }

    fn roundtrip(msg: &Message) {
        let encoded = encode_message(msg).unwrap();
        // first 4 bytes are the length header
        let payload_len = u32::from_be_bytes(encoded[..4].try_into().unwrap()) as usize;
        assert_eq!(payload_len, encoded.len() - 4);
        let decoded = decode_message(&encoded[4..]).unwrap();
        // compare via JSON since Message doesn't derive PartialEq
        let orig = serde_json::to_string(msg).unwrap();
        let back = serde_json::to_string(&decoded).unwrap();
        assert_eq!(orig, back);
    }

    #[test]
    fn test_roundtrip_switch_monitor() {
        roundtrip(&Message::SwitchMonitor {
            monitor_id: "DEL:U2720Q:SN123".into(),
            input_source_vcp: 0x11,
        });
    }

    #[test]
    fn test_roundtrip_switch_ack() {
        roundtrip(&Message::SwitchAck {
            monitor_id: "DEL:U2720Q:SN123".into(),
            success: true,
            error: None,
        });
        roundtrip(&Message::SwitchAck {
            monitor_id: "m".into(),
            success: false,
            error: Some("DDC timeout".into()),
        });
    }

    #[test]
    fn test_roundtrip_heartbeat() {
        roundtrip(&Message::Heartbeat {
            timestamp_ms: 1234567890,
        });
    }

    #[test]
    fn test_roundtrip_monitor_inventory() {
        roundtrip(&Message::MonitorInventory {
            monitors: vec![MonitorInfo {
                id: "DEL:U2720Q:SN123".into(),
                name: "Dell U2720Q".into(),
                manufacturer: "Dell".into(),
                model: "U2720Q".into(),
                serial: "SN123".into(),
                current_input_vcp: Some(0x0f),
                ddc_supported: true,
            }],
        });
    }

    #[test]
    fn test_roundtrip_request_inventory() {
        roundtrip(&Message::RequestInventory);
    }

    #[test]
    fn test_roundtrip_agent_hello() {
        roundtrip(&Message::AgentHello {
            agent_name: "MacBook".into(),
            version: PROTOCOL_VERSION,
        });
    }

    #[test]
    fn test_roundtrip_orchestrator_hello() {
        roundtrip(&Message::OrchestratorHello {
            version: PROTOCOL_VERSION,
        });
    }

    #[test]
    fn test_roundtrip_app_list() {
        roundtrip(&Message::AppList {
            apps: vec![AppInfo {
                name: "Firefox".into(),
                title: "GitHub - Mozilla Firefox".into(),
                pid: 1234,
                icon_base64: None,
            }],
        });
    }

    #[test]
    fn test_roundtrip_setup_query() {
        roundtrip(&Message::SetupQuery);
    }

    #[test]
    fn test_roundtrip_setup_info() {
        use std::collections::HashMap;
        // use a single entry to avoid HashMap ordering issues in JSON comparison
        let mut inputs = HashMap::new();
        inputs.insert("Windows-PC".into(), "DisplayPort1".into());
        roundtrip(&Message::SetupInfo {
            server_name: "Windows-PC".into(),
            os: "windows".into(),
            monitors: vec![MonitorInfo {
                id: "DEL:U2720Q:SN123".into(),
                name: "Dell U2720Q".into(),
                manufacturer: "Dell".into(),
                model: "U2720Q".into(),
                serial: "SN123".into(),
                current_input_vcp: Some(0x0f),
                ddc_supported: true,
            }],
            monitor_inputs: vec![SetupMonitorMapping {
                monitor_id: "DEL:U2720Q:SN123".into(),
                inputs,
            }],
        });
    }

    #[test]
    fn test_roundtrip_setup_test_switch() {
        roundtrip(&Message::SetupTestSwitch {
            monitor_id: "DEL:U2720Q:SN123".into(),
            input_vcp: 0x11,
        });
    }

    #[test]
    fn test_roundtrip_setup_test_switch_ack() {
        roundtrip(&Message::SetupTestSwitchAck {
            monitor_id: "DEL:U2720Q:SN123".into(),
            input_vcp: 0x11,
            success: true,
        });
    }

    #[test]
    fn test_decode_empty_frame() {
        let result = decode_message(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_invalid_json() {
        // type byte + garbage
        let frame = [0x01, b'{', b'b', b'a', b'd'];
        let result = decode_message(&frame);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_frame_structure() {
        let msg = Message::Heartbeat { timestamp_ms: 42 };
        let encoded = encode_message(&msg).unwrap();
        // first 4 bytes: length (BE)
        let len = u32::from_be_bytes(encoded[..4].try_into().unwrap());
        assert_eq!(len as usize, encoded.len() - 4);
        // 5th byte: type
        assert_eq!(encoded[4], 0x03);
        // rest: valid JSON
        let json: serde_json::Value = serde_json::from_slice(&encoded[5..]).unwrap();
        assert_eq!(json["Heartbeat"]["timestamp_ms"], 42);
    }

    #[tokio::test]
    async fn test_async_read_write_roundtrip() {
        let (mut client, mut server) = tokio::io::duplex(4096);

        let msg = Message::SwitchMonitor {
            monitor_id: "test-mon".into(),
            input_source_vcp: 0x0f,
        };

        write_message(&mut client, &msg).await.unwrap();
        drop(client); // close write end

        let decoded = read_message(&mut server).await.unwrap();
        let orig = serde_json::to_string(&msg).unwrap();
        let back = serde_json::to_string(&decoded).unwrap();
        assert_eq!(orig, back);
    }

    #[tokio::test]
    async fn test_async_read_oversized_length() {
        let (mut client, mut server) = tokio::io::duplex(4096);

        // write a frame header claiming 20MB payload
        let bad_len: u32 = 20 * 1024 * 1024;
        tokio::io::AsyncWriteExt::write_all(&mut client, &bad_len.to_be_bytes())
            .await
            .unwrap();
        drop(client);

        let result = read_message(&mut server).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("frame too large"), "got: {err}");
    }

    #[tokio::test]
    async fn test_async_multiple_messages() {
        let (mut client, mut server) = tokio::io::duplex(8192);

        let messages = vec![
            Message::AgentHello {
                agent_name: "test".into(),
                version: 1,
            },
            Message::Heartbeat { timestamp_ms: 100 },
            Message::RequestInventory,
        ];

        for msg in &messages {
            write_message(&mut client, msg).await.unwrap();
        }
        drop(client);

        for expected in &messages {
            let decoded = read_message(&mut server).await.unwrap();
            assert_eq!(
                serde_json::to_string(expected).unwrap(),
                serde_json::to_string(&decoded).unwrap(),
            );
        }
    }
}
