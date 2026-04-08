use softkvm_core::protocol::{discovery_response, DISCOVERY_MAGIC, DISCOVERY_PORT};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

/// run the UDP discovery responder
/// listens for SOFTKVM_DISCOVER broadcasts and responds with server info
pub async fn run_discovery_responder(
    server_name: String,
    version: String,
    listen_port: u16,
) -> anyhow::Result<()> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{DISCOVERY_PORT}")).await?;
    socket.set_broadcast(true)?;
    tracing::info!(port = DISCOVERY_PORT, "discovery responder listening");

    let mut buf = [0u8; 256];
    loop {
        let (len, src) = socket.recv_from(&mut buf).await?;
        let msg = &buf[..len];

        if msg == DISCOVERY_MAGIC {
            tracing::debug!(from = %src, "received discovery ping");

            // determine our IP from the source address's perspective
            let local_ip = local_ip_for(&src);
            let response = discovery_response(&server_name, &version, &local_ip, listen_port);

            if let Err(e) = socket.send_to(response.as_bytes(), src).await {
                tracing::warn!(error = %e, "failed to send discovery response");
            }
        }
    }
}

/// get the local IP address that would be used to reach a given remote address
fn local_ip_for(remote: &SocketAddr) -> String {
    // connect a UDP socket to determine the local interface
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(s) = socket {
        if s.connect(remote).is_ok() {
            if let Ok(local) = s.local_addr() {
                return local.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}
