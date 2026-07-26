//! Is anything listening, and is it SSH?
//!
//! Worth doing before a full connection: a closed port, a firewall black hole,
//! and a wrong password all end with "could not connect", but only one is fixed
//! by changing the port field, so the probe reports which it was.
//!
//! It reads the banner rather than assuming port 22 means SSH.

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use crate::ssh::SshResult;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Banner arrives immediately on a real sshd; this only bounds the wait for
/// something that accepted the connection and then went quiet.
const BANNER_TIMEOUT: Duration = Duration::from_secs(3);

/// What answered on the port.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub hostname: String,
    pub port: u16,
    /// Whether the TCP connection was accepted.
    pub reachable: bool,
    /// Whether the greeting looks like an SSH server.
    pub is_ssh: bool,
    /// The raw banner, e.g. `SSH-2.0-OpenSSH_9.6p1 Ubuntu-3ubuntu13.16`.
    pub banner: Option<String>,
    /// Round trip to a completed TCP handshake.
    pub latency_ms: Option<u64>,
    /// Plain-language result, ready to show.
    pub message: String,
}

/// Open a TCP connection and read whatever greets us.
pub async fn probe(hostname: &str, port: u16) -> SshResult<ProbeResult> {
    let started = Instant::now();
    let address = format!("{hostname}:{port}");

    let stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&address)).await {
        Err(_) => {
            return Ok(unreachable(
                hostname,
                port,
                format!(
                    "No response from {address} within {} seconds. The host may be off, \
                     or a firewall may be dropping the connection silently.",
                    CONNECT_TIMEOUT.as_secs()
                ),
            ))
        }
        Ok(Err(error)) => {
            // A refusal is a different problem from a timeout: something is
            // there, it just is not listening on this port.
            let message = if error.kind() == std::io::ErrorKind::ConnectionRefused {
                format!(
                    "{hostname} refused the connection on port {port}. \
                     Nothing is listening there — check the port, or that sshd is running."
                )
            } else {
                format!("Could not reach {address}: {error}")
            };
            return Ok(unreachable(hostname, port, message));
        }
        Ok(Ok(stream)) => stream,
    };

    let latency_ms = started.elapsed().as_millis() as u64;

    let mut buffer = [0u8; 255];
    let banner = match tokio::time::timeout(BANNER_TIMEOUT, {
        let mut stream = stream;
        async move { stream.read(&mut buffer).await.map(|read| (buffer, read)) }
    })
    .await
    {
        Ok(Ok((buffer, read))) if read > 0 => Some(
            String::from_utf8_lossy(&buffer[..read])
                .trim()
                .to_string(),
        ),
        _ => None,
    };

    let is_ssh = banner
        .as_deref()
        .map(|text| text.starts_with("SSH-"))
        .unwrap_or(false);

    let message = match (&banner, is_ssh) {
        (Some(text), true) => format!("Port {port} is open — {text}"),
        (Some(_), false) => format!(
            "Port {port} is open, but what answered is not an SSH server. \
             Check that the port is the right one for this host."
        ),
        (None, _) => format!(
            "Port {port} is open, but nothing identified itself. \
             It may not be an SSH server."
        ),
    };

    Ok(ProbeResult {
        hostname: hostname.to_string(),
        port,
        reachable: true,
        is_ssh,
        banner,
        latency_ms: Some(latency_ms),
        message,
    })
}

/// Just "is the port open?", with a caller-chosen timeout.
///
/// The heartbeat runs this against every saved host on a timer, so it skips
/// the banner read entirely — a completed TCP handshake is enough to say the
/// machine is up, and waiting for a greeting from each of twenty hosts would
/// make a 30-second cycle take longer than the cycle.
pub async fn reachable(hostname: &str, port: u16, timeout: Duration) -> (bool, Option<u64>) {
    let started = Instant::now();
    let address = format!("{hostname}:{port}");

    match tokio::time::timeout(timeout, TcpStream::connect(&address)).await {
        Ok(Ok(_stream)) => (true, Some(started.elapsed().as_millis() as u64)),
        _ => (false, None),
    }
}

fn unreachable(hostname: &str, port: u16, message: String) -> ProbeResult {
    ProbeResult {
        hostname: hostname.to_string(),
        port,
        reachable: false,
        is_ssh: false,
        banner: None,
        latency_ms: None,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_a_closed_port_without_erroring() {
        // Bind and immediately drop, so the port is almost certainly closed.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = probe("127.0.0.1", port).await.unwrap();
        assert!(!result.reachable);
        assert!(!result.is_ssh);
        // A closed port is a finding, not a failure — the UI needs the message.
        assert!(result.message.contains(&port.to_string()));
    }

    #[tokio::test]
    async fn recognises_an_ssh_banner() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::AsyncWriteExt;
                let _ = socket.write_all(b"SSH-2.0-OpenSSH_9.6p1 Ubuntu-3ubuntu13.16\r\n").await;
                let _ = socket.flush().await;
                // Hold the connection open long enough to be read.
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });

        let result = probe("127.0.0.1", port).await.unwrap();
        assert!(result.reachable);
        assert!(result.is_ssh);
        assert!(result.banner.unwrap().contains("OpenSSH_9.6p1"));
    }

    #[tokio::test]
    async fn an_open_port_that_is_not_ssh_is_flagged() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::AsyncWriteExt;
                let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\n").await;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });

        let result = probe("127.0.0.1", port).await.unwrap();
        assert!(result.reachable);
        assert!(!result.is_ssh, "an HTTP server must not pass as SSH");
    }
}
