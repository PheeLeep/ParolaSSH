//! Keeping connections alive between commands.
//!
//! Tauri commands are one-shot, but an SSH session is expensive to establish
//! and a terminal is worthless without one that persists. So an authenticated
//! session is parked here, keyed by host id, and later commands borrow it.
//!
//! One session per host is a deliberate ceiling: it keeps "is this host
//! connected?" a question with one answer, which is what the UI displays.
//!
//! Locks are `std::sync::Mutex` and are never held across an `await` — every
//! method clones the `Arc` it needs and releases the lock before doing I/O.
//! That is the whole discipline; breaking it would deadlock the runtime.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zeroize::Zeroizing;

use super::client::Session;
use super::power::Elevation;
use super::shell::ShellHandle;
use super::OsFamily;
use crate::ssh::{SshError, SshResult};

/// A connected host: the session, what we learned about it, and its terminal.
pub struct LiveSession {
    pub host_id: String,
    pub session: Session,
    pub os: OsFamily,
    pub os_detail: String,
    pub elevation: Elevation,
    pub fingerprint: Option<String>,
    pub connected_at: String,
    shell: Mutex<Option<Arc<ShellHandle>>>,
    /// The password this session authenticated with, when it used one.
    ///
    /// Kept so `sudo` can reuse it instead of asking for the same string a
    /// second time. Scoped to the session rather than the app: disconnecting
    /// drops it, which is a narrower lifetime than the "remember me" vault.
    login_password: Mutex<Option<Zeroizing<String>>>,
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
        Self {
            host_id,
            session,
            os,
            os_detail,
            elevation,
            fingerprint,
            connected_at,
            shell: Mutex::new(None),
            login_password: Mutex::new(None),
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

    pub fn shell(&self) -> Option<Arc<ShellHandle>> {
        self.shell.lock().ok().and_then(|shell| shell.clone())
    }

    pub fn set_shell(&self, handle: ShellHandle) -> Arc<ShellHandle> {
        let handle = Arc::new(handle);
        if let Ok(mut slot) = self.shell.lock() {
            *slot = Some(Arc::clone(&handle));
        }
        handle
    }

    pub fn take_shell(&self) -> Option<Arc<ShellHandle>> {
        self.shell.lock().ok().and_then(|mut shell| shell.take())
    }

    pub fn has_shell(&self) -> bool {
        self.shell
            .lock()
            .map(|shell| shell.is_some())
            .unwrap_or(false)
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
                if let Some(shell) = previous.take_shell() {
                    shell.close().await;
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
                if let Some(shell) = live.take_shell() {
                    shell.close().await;
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
