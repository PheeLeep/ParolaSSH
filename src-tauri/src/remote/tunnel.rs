//! Local port forwarding (`ssh -L`).
//!
//! Listens on a local TCP port and, for each incoming connection, opens a
//! `direct-tcpip` channel through the SSH session to the remote target, then
//! relays bytes bidirectionally until either side closes.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use russh::client::Msg;
use russh::Channel;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use super::registry::SessionRegistry;
use crate::ssh::{SshError, SshResult};

static NEXT_TUNNEL_ID: AtomicU64 = AtomicU64::new(1);

pub const TUNNEL_EVENT: &str = "tunnel://state";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub id: u64,
    pub host_id: String,
    pub local_port: u16,
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
    pub local_port: u16,
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
            local_port: self.local_port,
            remote_host: self.remote_host.clone(),
            remote_port: self.remote_port,
            active_connections: self.active_connections.load(Ordering::Relaxed),
        }
    }

    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

pub async fn open_local(
    app: AppHandle,
    registry: &SessionRegistry,
    host_id: String,
    local_port: u16,
    remote_host: String,
    remote_port: u16,
) -> SshResult<TunnelInfo> {
    // Verify the host is connected before binding.
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
        local_port: bound_port,
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

async fn relay(mut tcp: tokio::net::TcpStream, channel: Channel<Msg>) {
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
}
