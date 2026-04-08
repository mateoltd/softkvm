use std::sync::Arc;

use softkvm_core::protocol::{DaemonState, JsonRpcRequest, JsonRpcResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, RwLock};

/// commands the IPC server sends back to the main event loop
#[derive(Debug, Clone)]
pub enum IpcCommand {
    SwitchMachine(String),
    TestSwitch { monitor_id: String, input: String },
    SetFocusLock(bool),
    RescanMonitors,
}

/// shared state between the IPC server and the orchestrator main loop
#[derive(Clone)]
pub struct IpcState {
    pub daemon_state: Arc<RwLock<DaemonState>>,
    pub cmd_tx: mpsc::Sender<IpcCommand>,
}

/// default socket path per platform
pub fn default_socket_path() -> String {
    if cfg!(target_os = "windows") {
        r"\\.\pipe\SoftKvmIpc".into()
    } else {
        "/tmp/softkvm.sock".into()
    }
}

/// run the IPC server on the given socket path
pub async fn run_ipc_server(socket_path: &str, state: IpcState) -> std::io::Result<()> {
    // remove stale socket
    let _ = std::fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;
    tracing::info!(path = socket_path, "IPC server listening");

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                tracing::debug!(error = %e, "IPC client disconnected");
            }
        });
    }
}

async fn handle_client(stream: tokio::net::UnixStream, state: IpcState) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => dispatch(&req, &state).await,
            Err(e) => JsonRpcResponse::error(None, -32700, format!("parse error: {e}")),
        };

        let mut resp_json = serde_json::to_string(&response)?;
        resp_json.push('\n');
        writer.write_all(resp_json.as_bytes()).await?;
        writer.flush().await?;
    }

    Ok(())
}

async fn dispatch(req: &JsonRpcRequest, state: &IpcState) -> JsonRpcResponse {
    match req.method.as_str() {
        "get_state" => {
            let ds = state.daemon_state.read().await;
            JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(&*ds).unwrap_or_default(),
            )
        }
        "switch_machine" => {
            let machine = req
                .params
                .as_ref()
                .and_then(|p| p.get("machine"))
                .and_then(|v| v.as_str())
                .map(String::from);

            match machine {
                Some(name) => {
                    let _ = state
                        .cmd_tx
                        .send(IpcCommand::SwitchMachine(name.clone()))
                        .await;
                    JsonRpcResponse::success(
                        req.id.clone(),
                        serde_json::json!({"status": "ok", "machine": name}),
                    )
                }
                None => {
                    JsonRpcResponse::error(req.id.clone(), -32602, "missing params.machine".into())
                }
            }
        }
        "test_switch" => {
            let monitor_id = req
                .params
                .as_ref()
                .and_then(|p| p.get("monitor_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let input = req
                .params
                .as_ref()
                .and_then(|p| p.get("input"))
                .and_then(|v| v.as_str())
                .map(String::from);

            match (monitor_id, input) {
                (Some(mid), Some(inp)) => {
                    let _ = state
                        .cmd_tx
                        .send(IpcCommand::TestSwitch {
                            monitor_id: mid.clone(),
                            input: inp.clone(),
                        })
                        .await;
                    JsonRpcResponse::success(
                        req.id.clone(),
                        serde_json::json!({"status": "ok", "monitor_id": mid, "input": inp}),
                    )
                }
                _ => JsonRpcResponse::error(
                    req.id.clone(),
                    -32602,
                    "missing params.monitor_id or params.input".into(),
                ),
            }
        }
        "set_focus_lock" => {
            let locked = req
                .params
                .as_ref()
                .and_then(|p| p.get("locked"))
                .and_then(|v| v.as_bool());

            match locked {
                Some(val) => {
                    let _ = state.cmd_tx.send(IpcCommand::SetFocusLock(val)).await;
                    // update state immediately
                    state.daemon_state.write().await.focus_locked = val;
                    JsonRpcResponse::success(
                        req.id.clone(),
                        serde_json::json!({"status": "ok", "focus_locked": val}),
                    )
                }
                None => {
                    JsonRpcResponse::error(req.id.clone(), -32602, "missing params.locked".into())
                }
            }
        }
        "rescan_monitors" => {
            let _ = state.cmd_tx.send(IpcCommand::RescanMonitors).await;
            JsonRpcResponse::success(req.id.clone(), serde_json::json!({"status": "ok"}))
        }
        _ => JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            format!("unknown method: {}", req.method),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    fn test_state() -> (IpcState, mpsc::Receiver<IpcCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let daemon_state = Arc::new(RwLock::new(DaemonState {
            machines: vec![],
            monitors: vec![],
            active_machine: Some("Windows-PC".into()),
            focus_locked: false,
            deskflow_status: "running".into(),
        }));
        (
            IpcState {
                daemon_state,
                cmd_tx,
            },
            cmd_rx,
        )
    }

    fn temp_socket() -> String {
        format!("/tmp/softkvm-test-{}.sock", std::process::id())
    }

    async fn send_request(stream: &mut UnixStream, req: &str) -> String {
        let mut msg = req.to_string();
        msg.push('\n');
        stream.write_all(msg.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();

        // read response line
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).trim().to_string()
    }

    #[tokio::test]
    async fn test_ipc_get_state() {
        let socket = temp_socket();
        let (state, _cmd_rx) = test_state();

        let sock = socket.clone();
        let s = state.clone();
        tokio::spawn(async move {
            run_ipc_server(&sock, s).await.unwrap();
        });

        // give server time to bind
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let resp = send_request(
            &mut stream,
            r#"{"jsonrpc":"2.0","method":"get_state","id":1}"#,
        )
        .await;

        let parsed: JsonRpcResponse = serde_json::from_str(&resp).unwrap();
        assert!(parsed.error.is_none());
        let result = parsed.result.unwrap();
        assert_eq!(result["active_machine"], "Windows-PC");
        assert_eq!(result["focus_locked"], false);

        std::fs::remove_file(&socket).ok();
    }

    #[tokio::test]
    async fn test_ipc_switch_machine() {
        let socket = format!("/tmp/softkvm-test-switch-{}.sock", std::process::id());
        let (state, mut cmd_rx) = test_state();

        let sock = socket.clone();
        let s = state.clone();
        tokio::spawn(async move {
            run_ipc_server(&sock, s).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let resp = send_request(
            &mut stream,
            r#"{"jsonrpc":"2.0","method":"switch_machine","params":{"machine":"MacBook"},"id":2}"#,
        )
        .await;

        let parsed: JsonRpcResponse = serde_json::from_str(&resp).unwrap();
        assert!(parsed.error.is_none());

        // verify command was sent
        let cmd = cmd_rx.try_recv().unwrap();
        matches!(cmd, IpcCommand::SwitchMachine(ref name) if name == "MacBook");

        std::fs::remove_file(&socket).ok();
    }

    #[tokio::test]
    async fn test_ipc_set_focus_lock() {
        let socket = format!("/tmp/softkvm-test-lock-{}.sock", std::process::id());
        let (state, mut cmd_rx) = test_state();

        let sock = socket.clone();
        let s = state.clone();
        tokio::spawn(async move {
            run_ipc_server(&sock, s).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let resp = send_request(
            &mut stream,
            r#"{"jsonrpc":"2.0","method":"set_focus_lock","params":{"locked":true},"id":3}"#,
        )
        .await;

        let parsed: JsonRpcResponse = serde_json::from_str(&resp).unwrap();
        assert!(parsed.error.is_none());

        // verify state was updated
        assert!(state.daemon_state.read().await.focus_locked);

        // verify command was sent
        let cmd = cmd_rx.try_recv().unwrap();
        matches!(cmd, IpcCommand::SetFocusLock(true));

        std::fs::remove_file(&socket).ok();
    }

    #[tokio::test]
    async fn test_ipc_invalid_method() {
        let socket = format!("/tmp/softkvm-test-invalid-{}.sock", std::process::id());
        let (state, _cmd_rx) = test_state();

        let sock = socket.clone();
        let s = state.clone();
        tokio::spawn(async move {
            run_ipc_server(&sock, s).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let resp = send_request(
            &mut stream,
            r#"{"jsonrpc":"2.0","method":"nonexistent","id":4}"#,
        )
        .await;

        let parsed: JsonRpcResponse = serde_json::from_str(&resp).unwrap();
        assert!(parsed.error.is_some());
        assert_eq!(parsed.error.unwrap().code, -32601);

        std::fs::remove_file(&socket).ok();
    }

    #[tokio::test]
    async fn test_ipc_malformed_json() {
        let socket = format!("/tmp/softkvm-test-malformed-{}.sock", std::process::id());
        let (state, _cmd_rx) = test_state();

        let sock = socket.clone();
        let s = state.clone();
        tokio::spawn(async move {
            run_ipc_server(&sock, s).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let resp = send_request(&mut stream, "not valid json at all").await;

        let parsed: JsonRpcResponse = serde_json::from_str(&resp).unwrap();
        assert!(parsed.error.is_some());
        assert_eq!(parsed.error.unwrap().code, -32700);

        std::fs::remove_file(&socket).ok();
    }

    #[tokio::test]
    async fn test_ipc_multiple_clients() {
        let socket = format!("/tmp/softkvm-test-multi-{}.sock", std::process::id());
        let (state, _cmd_rx) = test_state();

        let sock = socket.clone();
        let s = state.clone();
        tokio::spawn(async move {
            run_ipc_server(&sock, s).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // connect two clients simultaneously
        let mut stream1 = UnixStream::connect(&socket).await.unwrap();
        let mut stream2 = UnixStream::connect(&socket).await.unwrap();

        let resp1 = send_request(
            &mut stream1,
            r#"{"jsonrpc":"2.0","method":"get_state","id":10}"#,
        )
        .await;
        let resp2 = send_request(
            &mut stream2,
            r#"{"jsonrpc":"2.0","method":"get_state","id":20}"#,
        )
        .await;

        let p1: JsonRpcResponse = serde_json::from_str(&resp1).unwrap();
        let p2: JsonRpcResponse = serde_json::from_str(&resp2).unwrap();
        assert!(p1.error.is_none());
        assert!(p2.error.is_none());
        // verify IDs are preserved
        assert_eq!(p1.id, Some(serde_json::json!(10)));
        assert_eq!(p2.id, Some(serde_json::json!(20)));

        std::fs::remove_file(&socket).ok();
    }
}
