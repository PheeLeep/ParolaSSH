//! Port forwarding - local (`ssh -L`) and remote (`ssh -R`).
//!
//! **Local**: listens on a local TCP port and, for each incoming connection,
//! opens a `direct-tcpip` channel through the SSH session to the remote
//! target, then relays bytes bidirectionally until either side closes.
//!
//! **Remote**: asks the server to listen on a port and, for each incoming
//! `forwarded-tcpip` channel, connects to a local target and relays.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use russh::client::Msg;
use russh::Channel;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex};

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
        }
    }

    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
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
    registry.require(&host_id)?;

    let addr: SocketAddr = ([127, 0, 0, 1], local_port).into();
    let listener = TcpListener::bind(addr).await.map_err(|error| {
        SshError::io(
            &format!("Could not listen on 127.0.0.1:{local_port}"),
            error,
        )
    })?;

    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(local_port);
    let id = NEXT_TUNNEL_ID.fetch_add(1, Ordering::Relaxed);
    let active_connections = Arc::new(AtomicU64::new(0));
    let (stop_tx, stop_rx) = watch::channel(false);

    let handle = TunnelHandle {
        id,
        host_id: host_id.clone(),
        direction: TunnelDirection::Local,
        local_port: bound_port,
        local_host: "127.0.0.1".into(),
        remote_host: remote_host.clone(),
        remote_port,
        active_connections: Arc::clone(&active_connections),
        stop: stop_tx,
    };

    let info = handle.info();

    let live = registry.require(&host_id)?;
    live.add_tunnel(handle);

    emit_tunnel_event(&app, &host_id, id, "opened");

    let rh = remote_host.clone();
    let rp = remote_port;
    let hid = host_id.clone();
    tokio::spawn(accept_loop(app, hid, id, listener, rh, rp, active_connections, stop_rx));

    Ok(info)
}

#[allow(clippy::too_many_arguments)]
async fn accept_loop(
    app: AppHandle,
    host_id: String,
    tunnel_id: u64,
    listener: TcpListener,
    remote_host: String,
    remote_port: u16,
    active_connections: Arc<AtomicU64>,
    mut stop_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            biased;
            _ = stop_rx.changed() => break,
            accepted = listener.accept() => {
                let (stream, _peer) = match accepted {
                    Ok(pair) => pair,
                    Err(_) => continue,
                };

                let registry = app.state::<SessionRegistry>();
                let live = match registry.get(&host_id) {
                    Some(live) => live,
                    None => break,
                };

                let channel = match live
                    .session
                    .channel_open_direct_tcpip(&remote_host, remote_port)
                    .await
                {
                    Ok(ch) => ch,
                    Err(_) => continue,
                };

                let count = Arc::clone(&active_connections);
                count.fetch_add(1, Ordering::Relaxed);
                let app2 = app.clone();
                let hid = host_id.clone();

                tokio::spawn(async move {
                    relay(stream, channel).await;
                    count.fetch_sub(1, Ordering::Relaxed);
                    emit_tunnel_event(&app2, &hid, tunnel_id, "connection_closed");
                });

                emit_tunnel_event(&app, &host_id, tunnel_id, "connection_opened");
            }
        }
    }

    let registry = app.state::<SessionRegistry>();
    if let Some(live) = registry.get(&host_id) {
        live.remove_tunnel(tunnel_id);
    }
    emit_tunnel_event(&app, &host_id, tunnel_id, "closed");
}

// ── Remote forwarding (ssh -R) ──────────────────────────────────────────

/// Where to connect locally when a `forwarded-tcpip` channel arrives.
pub struct RemoteTarget {
    pub tunnel_id: u64,
    pub local_host: String,
    pub local_port: u16,
    pub active_connections: Arc<AtomicU64>,
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
    let live = registry.require(&host_id)?;

    let bound_port = live
        .session
        .tcpip_forward(&remote_bind_host, remote_port)
        .await?;

    let id = NEXT_TUNNEL_ID.fetch_add(1, Ordering::Relaxed);
    let active_connections = Arc::new(AtomicU64::new(0));
    let (stop_tx, _stop_rx) = watch::channel(false);

    let handle = TunnelHandle {
        id,
        host_id: host_id.clone(),
        direction: TunnelDirection::Remote,
        local_port,
        local_host: local_host.clone(),
        remote_host: remote_bind_host.clone(),
        remote_port: bound_port,
        active_connections: Arc::clone(&active_connections),
        stop: stop_tx,
    };

    let info = handle.info();
    live.add_tunnel(handle);

    {
        let map = live.remote_targets();
        let mut targets = map.lock().await;
        targets.insert(
            (remote_bind_host.clone(), bound_port as u32),
            RemoteTarget {
                tunnel_id: id,
                local_host,
                local_port,
                active_connections,
            },
        );
    }

    if !live.remote_dispatch_started() {
        live.mark_remote_dispatch_started();
        if let Some(rx) = live.session.take_forwarded_rx().await {
            let targets = live.remote_targets();
            let app2 = app.clone();
            let hid = host_id.clone();
            tokio::spawn(remote_dispatch(app2, hid, rx, targets));
        }
    }

    emit_tunnel_event(&app, &host_id, id, "opened");
    Ok(info)
}

pub async fn close_remote(
    registry: &SessionRegistry,
    host_id: &str,
    tunnel_id: u64,
) -> SshResult<()> {
    let live = registry.require(host_id)?;

    let handle = live
        .remove_tunnel(tunnel_id)
        .ok_or_else(|| SshError::invalid("That tunnel does not exist."))?;

    if handle.direction != TunnelDirection::Remote {
        live.add_tunnel(handle);
        return Err(SshError::invalid("That is not a remote tunnel."));
    }

    let key = (handle.remote_host.clone(), handle.remote_port as u32);
    live.remote_targets().lock().await.remove(&key);

    let _ = live
        .session
        .cancel_tcpip_forward(&handle.remote_host, handle.remote_port)
        .await;

    Ok(())
}

async fn remote_dispatch(
    app: AppHandle,
    host_id: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ForwardedChannel>,
    targets: RemoteForwardMap,
) {
    while let Some(fwd) = rx.recv().await {
        let key = (fwd.connected_address.clone(), fwd.connected_port);
        let target = targets.lock().await;
        let Some(entry) = target.get(&key) else {
            continue;
        };

        let local_addr = format!("{}:{}", entry.local_host, entry.local_port);
        let tunnel_id = entry.tunnel_id;
        let count = Arc::clone(&entry.active_connections);
        drop(target);

        let stream = match TcpStream::connect(&local_addr).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        count.fetch_add(1, Ordering::Relaxed);
        let app2 = app.clone();
        let hid = host_id.clone();

        emit_tunnel_event(&app, &host_id, tunnel_id, "connection_opened");

        tokio::spawn(async move {
            relay(stream, fwd.channel).await;
            count.fetch_sub(1, Ordering::Relaxed);
            emit_tunnel_event(&app2, &hid, tunnel_id, "connection_closed");
        });
    }
}

// ── Shared relay ────────────────────────────────────────────────────────

async fn relay(mut tcp: TcpStream, channel: Channel<Msg>) {
    let (mut ssh_read, ssh_write) = channel.split();
    let (mut tcp_read, mut tcp_write) = tcp.split();

    let local_to_remote = async {
        let mut buf = [0u8; 32768];
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
    };

    let remote_to_local = async {
        loop {
            match ssh_read.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    if tcp_write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(russh::ChannelMsg::Eof | russh::ChannelMsg::Close) | None => break,
                _ => {}
            }
        }
    };

    tokio::join!(local_to_remote, remote_to_local);
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
}
