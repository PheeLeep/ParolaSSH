//! Keeping connections alive between commands.
//!
//! Tauri commands are one-shot, but an SSH session is expensive to establish
//! and a terminal is worthless without one that persists. So an authenticated
//! session is parked here, keyed by host id, and later commands borrow it.
//!
//! One session per host is a deliberate ceiling: it keeps "is this host
//! connected?" a question with one answer, which is what the UI displays.
//!
//! State locks are `std::sync::Mutex` and are never held across an `await` —
//! every method clones the `Arc` it needs and releases the lock before doing
//! I/O. That is the whole discipline; breaking it would deadlock the runtime.
//! The one exception is `shell_open`, which exists precisely to be held across
//! an await and is therefore a `tokio::sync::Mutex`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zeroize::Zeroizing;

use super::client::{NegotiatedCrypto, Session};
use super::metrics::CpuTimes;
use super::power::Elevation;
use super::shell::ShellHandle;
use super::stream::StreamHandle;
use super::OsFamily;
use crate::ssh::{SshError, SshResult};

/// How many long-running streams one host may hold at once.
///
/// The real use is one followed log per visible pane, so this is headroom,
/// not a budget — its job is to stop a leak from accumulating quietly.
pub const MAX_STREAMS_PER_HOST: usize = 4;

/// A connected host: the session, what we learned about it, and its terminal.
pub struct LiveSession {
    pub host_id: String,
    pub session: Session,
    pub os: OsFamily,
    pub os_detail: String,
    pub elevation: Elevation,
    pub fingerprint: Option<String>,
    /// What the first key exchange negotiated — the audit tab's tier 0.
    pub negotiated: Option<NegotiatedCrypto>,
    pub connected_at: String,
    /// Every open shell on this host, keyed by shell id.
    ///
    /// A host holds several terminals — one tailing a log, one running
    /// commands — and they all ride the single authenticated connection as
    /// separate channels. That is what OpenSSH's `ControlMaster` does, and it
    /// means the second terminal costs no handshake and no second password.
    shells: Mutex<HashMap<u64, Arc<ShellHandle>>>,
    /// Long-running command streams (a followed log, for now), keyed by
    /// stream id. Separate from `shells` because they have no PTY and no
    /// input, but they are drained at the same four moments so a followed
    /// journal cannot outlive its session.
    streams: Mutex<HashMap<u64, Arc<StreamHandle>>>,
    /// Serialises opening a shell, so the cap below cannot be raced past.
    /// A tokio mutex because it is held across `await`.
    shell_open: tokio::sync::Mutex<()>,
    /// The password this session authenticated with, when it used one.
    ///
    /// Kept so `sudo` can reuse it instead of asking for the same string a
    /// second time. Scoped to the session rather than the app: disconnecting
    /// drops it, which is a narrower lifetime than the "remember me" vault.
    login_password: Mutex<Option<Zeroizing<String>>>,
    /// The last `/proc/stat` reading, so CPU percentage can be a delta
    /// between polls. Read and written only after an exec completes — never
    /// held across an await. Naturally reset by reconnection.
    prev_cpu: Mutex<Option<CpuTimes>>,
}

impl LiveSession {
    pub fn new(
        host_id: String,
        session: Session,
        os: OsFamily,
        os_detail: String,
        elevation: Elevation,
        connected_at: String,
    ) -> Self {
        let fingerprint = session.fingerprint.clone();
        let negotiated = session.negotiated.clone();
        Self {
            host_id,
            session,
            os,
            os_detail,
            elevation,
            fingerprint,
            negotiated,
            connected_at,
            shells: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            shell_open: tokio::sync::Mutex::new(()),
            login_password: Mutex::new(None),
            prev_cpu: Mutex::new(None),
        }
    }

    /// Remember the password this session logged in with, for sudo.
    pub fn set_login_password(&self, password: Zeroizing<String>) {
        if let Ok(mut slot) = self.login_password.lock() {
            *slot = Some(password);
        }
    }

    pub fn login_password(&self) -> Option<Zeroizing<String>> {
        self.login_password.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn has_login_password(&self) -> bool {
        self.login_password
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false)
    }

    /// The CPU reading from the previous metrics sample, if any.
    pub fn prev_cpu(&self) -> Option<CpuTimes> {
        self.prev_cpu.lock().ok().and_then(|slot| *slot)
    }

    pub fn set_prev_cpu(&self, times: CpuTimes) {
        if let Ok(mut slot) = self.prev_cpu.lock() {
            *slot = Some(times);
        }
    }

    /// Hold this for the duration of an open, so the cap is enforced atomically.
    pub async fn lock_shell_open(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.shell_open.lock().await
    }

    /// Look up one shell. Returns `None` once it has been closed, which is
    /// how a late write from an unmounting pane fails harmlessly.
    pub fn shell(&self, shell_id: u64) -> Option<Arc<ShellHandle>> {
        self.shells
            .lock()
            .ok()
            .and_then(|shells| shells.get(&shell_id).cloned())
    }

    pub fn add_shell(&self, handle: ShellHandle) -> Arc<ShellHandle> {
        let handle = Arc::new(handle);
        if let Ok(mut shells) = self.shells.lock() {
            shells.insert(handle.id, Arc::clone(&handle));
        }
        handle
    }

    /// Remove one shell, returning it so the caller can close the channel.
    ///
    /// Removing by id is what stops a pane tearing down late from closing a
    /// shell that is not its own — an unknown id is a no-op, not an error.
    pub fn remove_shell(&self, shell_id: u64) -> Option<Arc<ShellHandle>> {
        self.shells
            .lock()
            .ok()
            .and_then(|mut shells| shells.remove(&shell_id))
    }

    /// Empty the map, returning everything that was in it.
    ///
    /// Used when the session goes away: leaving entries behind would hold
    /// channel handles for the lifetime of the app.
    pub fn drain_shells(&self) -> Vec<Arc<ShellHandle>> {
        self.shells
            .lock()
            .map(|mut shells| shells.drain().map(|(_, shell)| shell).collect())
            .unwrap_or_default()
    }

    /// Open shell ids, oldest first, so the UI can rebuild its tabs.
    pub fn shell_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .shells
            .lock()
            .map(|shells| shells.keys().copied().collect())
            .unwrap_or_default();
        ids.sort_unstable();
        ids
    }

    pub fn shell_count(&self) -> usize {
        self.shells.lock().map(|shells| shells.len()).unwrap_or(0)
    }

    /// Park a stream. The cap is checked by the caller *before* the channel
    /// is opened — refusing after would mean closing a command already
    /// running remotely. A race between two opens can overshoot by one,
    /// which a leak-stop (unlike a security boundary) can tolerate.
    pub fn add_stream(&self, handle: StreamHandle) -> Arc<StreamHandle> {
        let handle = Arc::new(handle);
        if let Ok(mut streams) = self.streams.lock() {
            streams.insert(handle.id, Arc::clone(&handle));
        }
        handle
    }

    pub fn stream_count(&self) -> usize {
        self.streams.lock().map(|streams| streams.len()).unwrap_or(0)
    }

    /// Remove one stream, returning it so the caller can close the channel.
    /// An unknown id is a no-op, for the same reason as `remove_shell`.
    pub fn remove_stream(&self, stream_id: u64) -> Option<Arc<StreamHandle>> {
        self.streams
            .lock()
            .ok()
            .and_then(|mut streams| streams.remove(&stream_id))
    }

    /// Empty the map, returning everything that was in it.
    pub fn drain_streams(&self) -> Vec<Arc<StreamHandle>> {
        self.streams
            .lock()
            .map(|mut streams| streams.drain().map(|(_, stream)| stream).collect())
            .unwrap_or_default()
    }
}

#[derive(Default)]
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, Arc<LiveSession>>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Park a session, replacing and disconnecting any previous one for the
    /// same host so a reconnect never leaks the connection it replaced.
    pub fn insert(&self, live: LiveSession) -> Arc<LiveSession> {
        let live = Arc::new(live);
        let previous = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.insert(live.host_id.clone(), Arc::clone(&live)));

        if let Some(previous) = previous {
            tokio::spawn(async move {
                for shell in previous.drain_shells() {
                    shell.close().await;
                }
                for stream in previous.drain_streams() {
                    stream.close().await;
                }
                previous.session.close().await;
            });
        }

        live
    }

    pub fn get(&self, host_id: &str) -> Option<Arc<LiveSession>> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(host_id).cloned())
    }

    /// Fetch a session, or explain that there is not one.
    pub fn require(&self, host_id: &str) -> SshResult<Arc<LiveSession>> {
        self.get(host_id).ok_or_else(|| {
            SshError::invalid("Not connected to that host. Connect first, then try again.")
        })
    }

    pub fn remove(&self, host_id: &str) -> Option<Arc<LiveSession>> {
        self.sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(host_id))
    }

    pub fn is_connected(&self, host_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|sessions| sessions.contains_key(host_id))
            .unwrap_or(false)
    }

    pub fn connected_ids(&self) -> Vec<String> {
        self.sessions
            .lock()
            .map(|sessions| sessions.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Disconnect one host. Closing the shell first lets the remote side see a
    /// clean channel close rather than the whole transport vanishing.
    pub async fn disconnect(&self, host_id: &str) -> bool {
        match self.remove(host_id) {
            Some(live) => {
                // Every shell, not just the active one — an entry left behind
                // holds a channel handle for the lifetime of the app.
                for shell in live.drain_shells() {
                    shell.close().await;
                }
                for stream in live.drain_streams() {
                    stream.close().await;
                }
                live.session.close().await;
                true
            }
            None => false,
        }
    }

    /// Drop everything — used on app exit.
    pub async fn disconnect_all(&self) {
        for host_id in self.connected_ids() {
            self.disconnect(&host_id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_which_hosts_are_connected() {
        let registry = SessionRegistry::new();
        assert!(!registry.is_connected("h-1"));
        assert!(registry.get("h-1").is_none());
        assert!(registry.require("h-1").is_err());
        assert!(registry.connected_ids().is_empty());
    }
}
