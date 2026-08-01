//! Is anything listening, and is it SSH?
//!
//! Worth doing before a full connection: a closed port, a firewall black hole,
//! and a wrong password all end with "could not connect", but only one is fixed
//! by changing the port field, so the probe reports which it was.
//!
//! It reads the banner rather than assuming port 22 means SSH. When a username
//! is provided it also performs a throwaway SSH handshake to discover which
//! authentication methods the server advertises.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::client;
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use crate::ssh::SshResult;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Banner arrives immediately on a real sshd; this only bounds the wait for
/// something that accepted the connection and then went quiet.
const BANNER_TIMEOUT: Duration = Duration::from_secs(3);
/// The auth-method probe is a full SSH handshake; allow more than the banner.
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Auth methods the server will accept, discovered via `authenticate_none`.
    /// `None` when no username was given or the server is not SSH.
    pub auth_methods: Option<Vec<String>>,
    /// SSH handshake log lines from the probe connection.
    pub logs: Vec<String>,
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
                     Nothing is listening there - check the port, or that sshd is running."
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
        (Some(text), true) => format!("Port {port} is open - {text}"),
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
        auth_methods: None,
        logs: Vec::new(),
    })
}

/// Just "is the port open?", with a caller-chosen timeout.
///
/// The heartbeat runs this against every saved host on a timer, so it skips
/// the banner read entirely - a completed TCP handshake is enough to say the
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
        auth_methods: None,
        logs: Vec::new(),
    }
}

// ── Auth method detection ───────────────────────────────────────────────

/// Minimal handler that accepts any host key - this is a throwaway probe,
/// not a real session, and no credential is ever sent.
struct ProbeHandler {
    accepted: Arc<Mutex<bool>>,
    logs: Arc<Mutex<Vec<String>>>,
}

impl client::Handler for ProbeHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        if let Ok(mut slot) = self.accepted.lock() {
            *slot = true;
        }
        if let Ok(mut log) = self.logs.lock() {
            log.push(format!("Host key: {} {}", key.algorithm(), key.fingerprint(Default::default())));
        }
        Ok(true)
    }

    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if let Ok(mut log) = self.logs.lock() {
            for line in banner.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    log.push(format!("Banner: {trimmed}"));
                }
            }
        }
        Ok(())
    }

    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if let Ok(mut log) = self.logs.lock() {
            log.push(format!("KEX: {}", names.kex.as_ref()));
            log.push(format!("Host key algorithm: {}", names.key));
            log.push(format!("Cipher: {}", names.cipher.as_ref()));
            log.push(format!("Client MAC: {}", names.client_mac.as_ref()));
            log.push(format!("Server MAC: {}", names.server_mac.as_ref()));
            if names.strict_kex() {
                log.push("Strict KEX: enabled".to_string());
            }
        }
        Ok(())
    }
}

pub struct AuthProbeResult {
    pub methods: Vec<String>,
    pub logs: Vec<String>,
}

/// Connect, call `authenticate_none`, and read the methods the server will
/// accept. The connection is dropped immediately after.
pub async fn detect_auth_methods(
    hostname: &str,
    port: u16,
    username: &str,
) -> Option<AuthProbeResult> {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let handler = ProbeHandler {
        accepted: Arc::new(Mutex::new(false)),
        logs: Arc::clone(&logs),
    };

    let config = Arc::new(client::Config {
        inactivity_timeout: Some(AUTH_PROBE_TIMEOUT),
        ..Default::default()
    });

    let address = (hostname.to_string(), port);
    let connect = client::connect(config, address, handler);
    let mut handle = match tokio::time::timeout(AUTH_PROBE_TIMEOUT, connect).await {
        Ok(Ok(h)) => h,
        _ => return None,
    };

    let result = handle.authenticate_none(username).await.ok()?;

    let methods = match result {
        client::AuthResult::Success => vec!["none".to_string()],
        client::AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods
            .iter()
            .map(|m| <&str>::from(m).to_string())
            .collect(),
    };

    if let Ok(mut log) = logs.lock() {
        let names: Vec<&str> = methods.iter().map(|s| s.as_str()).collect();
        log.push(format!("Auth methods: {}", names.join(", ")));
    }

    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "en")
        .await;

    let collected = logs.lock().ok().map(|l| l.clone()).unwrap_or_default();
    Some(AuthProbeResult {
        methods,
        logs: collected,
    })
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
        // A closed port is a finding, not a failure - the UI needs the message.
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
