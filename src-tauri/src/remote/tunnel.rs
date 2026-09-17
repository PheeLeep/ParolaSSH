//! Port forwarding - local (`ssh -L`) and remote (`ssh -R`).
//!
//! **Local**: listens on a local TCP port and, for each incoming connection,
//! opens a `direct-tcpip` channel through the SSH session to the remote
//! target, then relays bytes bidirectionally until either side closes.
//!
//! **Remote**: asks the server to listen on a port and, for each incoming
//! `forwarded-tcpip` channel, connects to a local target and relays.
//!
//! Every connection runs in its own task, so a slow channel open or an
//! unreachable target never holds up the next connection.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch, Mutex};

use super::client::ForwardedChannel;
use super::registry::SessionRegistry;
use crate::ssh::{SshError, SshResult};

static NEXT_TUNNEL_ID: AtomicU64 = AtomicU64::new(1);

pub const TUNNEL_EVENT: &str = "tunnel://state";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelDirection {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub id: u64,
    pub host_id: String,
    pub direction: TunnelDirection,
    pub local_port: u16,
    pub local_host: String,
    pub remote_host: String,
    pub remote_port: u16,
    pub active_connections: u64,
    /// Why the most recent connection failed; cleared by the next that succeeds.
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelEvent {
    pub host_id: String,
    pub tunnel_id: u64,
    pub kind: String,
}

pub struct TunnelHandle {
    pub id: u64,
    pub host_id: String,
    pub direction: TunnelDirection,
    pub local_port: u16,
    pub local_host: String,
    pub remote_host: String,
    pub remote_port: u16,
    active_connections: Arc<AtomicU64>,
    last_error: Arc<StdMutex<Option<String>>>,
    stop: watch::Sender<bool>,
}

impl TunnelHandle {
    pub fn info(&self) -> TunnelInfo {
        TunnelInfo {
            id: self.id,
            host_id: self.host_id.clone(),
            direction: self.direction,
            local_port: self.local_port,
            local_host: self.local_host.clone(),
            remote_host: self.remote_host.clone(),
            remote_port: self.remote_port,
            active_connections: self.active_connections.load(Ordering::Relaxed),
            last_error: self.last_error.lock().ok().and_then(|e| e.clone()),
        }
    }

    /// Stop accepting and end every connection still flowing through.
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

/// What a connection task needs to report on its tunnel.
#[derive(Clone)]
struct Tracker {
    app: AppHandle,
    host_id: String,
    tunnel_id: u64,
    active_connections: Arc<AtomicU64>,
    last_error: Arc<StdMutex<Option<String>>>,
    stop: watch::Receiver<bool>,
}

impl Tracker {
    async fn run(&self, stream: TcpStream, channel: Channel<Msg>) {
        if let Ok(mut error) = self.last_error.lock() {
            *error = None;
        }
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.emit("connection_opened");
        relay(stream, channel, self.stop.clone()).await;
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
        self.emit("connection_closed");
    }

    fn fail(&self, message: String) {
        if let Ok(mut error) = self.last_error.lock() {
            *error = Some(message);
        }
        self.emit("connection_failed");
    }

    fn emit(&self, kind: &str) {
        emit_tunnel_event(&self.app, &self.host_id, self.tunnel_id, kind);
    }
}

/// A handle plus the tracker its connection tasks share.
fn new_tunnel(
    app: &AppHandle,
    host_id: &str,
    direction: TunnelDirection,
    local: (String, u16),
    remote: (String, u16),
) -> (TunnelHandle, Tracker) {
    let id = NEXT_TUNNEL_ID.fetch_add(1, Ordering::Relaxed);
    let active_connections = Arc::new(AtomicU64::new(0));
    let last_error = Arc::new(StdMutex::new(None));
    let (stop_tx, stop_rx) = watch::channel(false);

    let tracker = Tracker {
        app: app.clone(),
        host_id: host_id.to_string(),
        tunnel_id: id,
        active_connections: Arc::clone(&active_connections),
        last_error: Arc::clone(&last_error),
        stop: stop_rx,
    };
    let handle = TunnelHandle {
        id,
        host_id: host_id.to_string(),
        direction,
        local_host: local.0,
        local_port: local.1,
        remote_host: remote.0,
        remote_port: remote.1,
        active_connections,
        last_error,
        stop: stop_tx,
    };
    (handle, tracker)
}

// ── Local forwarding (ssh -L) ───────────────────────────────────────────

pub async fn open_local(
    app: AppHandle,
    registry: &SessionRegistry,
    host_id: String,
    local_port: u16,
    remote_host: String,
    remote_port: u16,
) -> SshResult<TunnelInfo> {
    let live = registry.require(&host_id)?;

    let addr: SocketAddr = ([127, 0, 0, 1], local_port).into();
    let listener = TcpListener::bind(addr).await.map_err(|error| {
        SshError::io(&format!("Could not listen on 127.0.0.1:{local_port}"), error)
    })?;
    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(local_port);

    let (handle, tracker) = new_tunnel(
        &app,
        &host_id,
        TunnelDirection::Local,
        ("127.0.0.1".into(), bound_port),
        (remote_host.clone(), remote_port),
    );
    let info = handle.info();
    live.add_tunnel(handle);

    tracker.emit("opened");
    tokio::spawn(accept_loop(tracker, listener, remote_host, remote_port));

    Ok(info)
}

async fn accept_loop(
    tracker: Tracker,
    listener: TcpListener,
    remote_host: String,
    remote_port: u16,
) {
    let mut stop = tracker.stop.clone();
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { continue };
                let registry = tracker.app.state::<SessionRegistry>();
                let Some(live) = registry.get(&tracker.host_id) else { break };

                let tracker = tracker.clone();
                let host = remote_host.clone();
                tokio::spawn(async move {
                    let opened = live.session.channel_open_direct_tcpip(&host, remote_port).await;
                    drop(live);
                    match opened {
                        Ok(channel) => tracker.run(stream, channel).await,
                        Err(error) => tracker.fail(error.to_string()),
                    }
                });
            }
        }
    }

    let registry = tracker.app.state::<SessionRegistry>();
    if let Some(live) = registry.get(&tracker.host_id) {
        live.remove_tunnel(tracker.tunnel_id);
    }
    tracker.emit("closed");
}

// ── Remote forwarding (ssh -R) ──────────────────────────────────────────

/// Where to connect locally when a `forwarded-tcpip` channel arrives.
pub struct RemoteTarget {
    local_host: String,
    local_port: u16,
    tracker: Tracker,
}

/// Per-session map of `(bind_address, bind_port)` to local targets.
pub type RemoteForwardMap = Arc<Mutex<HashMap<(String, u32), RemoteTarget>>>;

pub fn new_remote_forward_map() -> RemoteForwardMap {
    Arc::new(Mutex::new(HashMap::new()))
}

pub async fn open_remote(
    app: AppHandle,
    registry: &SessionRegistry,
    host_id: String,
    remote_bind_host: String,
    remote_port: u16,
    local_host: String,
    local_port: u16,
) -> SshResult<TunnelInfo> {
    if local_port == 0 {
        return Err(SshError::invalid("Choose the port on this machine to forward to."));
    }
    let live = registry.require(&host_id)?;

    let bound_port = live.session.tcpip_forward(&remote_bind_host, remote_port).await?;

    let (handle, tracker) = new_tunnel(
        &app,
        &host_id,
        TunnelDirection::Remote,
        (local_host.clone(), local_port),
        (remote_bind_host.clone(), bound_port),
    );
    let info = handle.info();
    live.add_tunnel(handle);

    live.remote_targets().lock().await.insert(
        (remote_bind_host, u32::from(bound_port)),
        RemoteTarget { local_host, local_port, tracker: tracker.clone() },
    );

    // Only the first remote tunnel on a session gets the receiver.
    if let Some(rx) = live.session.take_forwarded_rx().await {
        tokio::spawn(remote_dispatch(rx, live.remote_targets()));
    }

    tracker.emit("opened");
    Ok(info)
}

async fn remote_dispatch(mut rx: mpsc::UnboundedReceiver<ForwardedChannel>, targets: RemoteForwardMap) {
    while let Some(fwd) = rx.recv().await {
        let found = {
            let targets = targets.lock().await;
            find_target(&targets, &fwd.connected_address, fwd.connected_port)
                .map(|t| (t.local_host.clone(), t.local_port, t.tracker.clone()))
        };
        let Some((host, port, tracker)) = found else {
            let _ = fwd.channel.close().await;
            continue;
        };

        tokio::spawn(async move {
            match TcpStream::connect((host.as_str(), port)).await {
                Ok(stream) => tracker.run(stream, fwd.channel).await,
                Err(error) => {
                    let _ = fwd.channel.close().await;
                    tracker.fail(format!("Could not reach {host}:{port} on this machine - {error}"));
                }
            }
        });
    }
}

/// Exact bind address first. Servers that report a different address than we
/// asked for still match on the port, as long as only one forward uses it.
fn find_target<'a, T>(
    targets: &'a HashMap<(String, u32), T>,
    address: &str,
    port: u32,
) -> Option<&'a T> {
    if let Some(target) = targets.get(&(address.to_string(), port)) {
        return Some(target);
    }
    let mut same_port = targets.iter().filter(|((_, p), _)| *p == port);
    match (same_port.next(), same_port.next()) {
        (Some((_, target)), None) => Some(target),
        _ => None,
    }
}

// ── Closing ─────────────────────────────────────────────────────────────

pub async fn close(
    app: &AppHandle,
    registry: &SessionRegistry,
    host_id: &str,
    tunnel_id: u64,
) -> SshResult<()> {
    let live = registry.require(host_id)?;
    let handle = live
        .remove_tunnel(tunnel_id)
        .ok_or_else(|| SshError::invalid("That tunnel is not running."))?;
    handle.stop();

    if handle.direction == TunnelDirection::Remote {
        // Unroute first, so nothing arriving during the cancel is relayed.
        let key = (handle.remote_host.clone(), u32::from(handle.remote_port));
        live.remote_targets().lock().await.remove(&key);
        let _ = live
            .session
            .cancel_tcpip_forward(&handle.remote_host, handle.remote_port)
            .await;
        emit_tunnel_event(app, host_id, tunnel_id, "closed");
    }
    Ok(())
}

// ── Shared relay ────────────────────────────────────────────────────────

/// Pipe bytes both ways until the server closes the channel or `stop` fires.
/// An EOF from either side is passed on as a half-close, not a teardown.
pub async fn relay(mut tcp: TcpStream, channel: Channel<Msg>, mut stop: watch::Receiver<bool>) {
    let (mut ssh_read, ssh_write) = channel.split();
    let (mut tcp_read, mut tcp_write) = tcp.split();

    let local_to_remote = async {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            let n = match tcp_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if ssh_write.data(&buf[..n]).await.is_err() {
                break;
            }
        }
        let _ = ssh_write.eof().await;
        // The server may still be answering; its close ends the relay.
        std::future::pending::<()>().await;
    };

    let remote_to_local = async {
        while let Some(msg) = ssh_read.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    if tcp_write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                ChannelMsg::Eof => {
                    let _ = tcp_write.shutdown().await;
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
    };

    tokio::select! {
        _ = local_to_remote => {}
        _ = remote_to_local => {}
        _ = stop.changed() => {}
    }
    let _ = ssh_write.close().await;
}

fn emit_tunnel_event(app: &AppHandle, host_id: &str, tunnel_id: u64, kind: &str) {
    let _ = app.emit(
        TUNNEL_EVENT,
        TunnelEvent {
            host_id: host_id.to_string(),
            tunnel_id,
            kind: kind.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(entries: &[(&str, u32, u64)]) -> HashMap<(String, u32), u64> {
        entries
            .iter()
            .map(|(host, port, id)| ((host.to_string(), *port), *id))
            .collect()
    }

    #[test]
    fn tunnel_id_increments() {
        let a = NEXT_TUNNEL_ID.fetch_add(1, Ordering::Relaxed);
        let b = NEXT_TUNNEL_ID.fetch_add(1, Ordering::Relaxed);
        assert!(b > a);
    }

    #[test]
    fn direction_serializes_lowercase() {
        let json = serde_json::to_string(&TunnelDirection::Remote).unwrap();
        assert_eq!(json, "\"remote\"");
        let json = serde_json::to_string(&TunnelDirection::Local).unwrap();
        assert_eq!(json, "\"local\"");
    }

    #[test]
    fn target_matches_the_exact_bind_address() {
        let map = targets(&[("127.0.0.1", 8080, 1), ("0.0.0.0", 8080, 2)]);
        assert_eq!(find_target(&map, "0.0.0.0", 8080), Some(&2));
        assert_eq!(find_target(&map, "127.0.0.1", 8080), Some(&1));
    }

    #[test]
    fn a_rewritten_address_falls_back_to_a_unique_port() {
        let map = targets(&[("localhost", 8080, 1), ("127.0.0.1", 9090, 2)]);
        assert_eq!(find_target(&map, "127.0.0.1", 8080), Some(&1));
    }

    #[test]
    fn an_ambiguous_port_matches_nothing() {
        let map = targets(&[("127.0.0.1", 8080, 1), ("0.0.0.0", 8080, 2)]);
        assert_eq!(find_target(&map, "::1", 8080), None);
    }

    #[test]
    fn an_unknown_port_matches_nothing() {
        let map = targets(&[("127.0.0.1", 8080, 1)]);
        assert_eq!(find_target(&map, "127.0.0.1", 8081), None);
    }
}
