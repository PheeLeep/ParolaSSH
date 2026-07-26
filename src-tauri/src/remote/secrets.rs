//! Passwords, for as long as the app is running and no longer.
//!
//! Deliberately not a keychain: durable storage means three platform backends
//! (Credential Manager, Keychain, a Secret Service that may not be installed),
//! and getting that subtly wrong is worse than not offering it. So the vault is
//! a `HashMap` that dies with the process, and the UI says "until you quit".
//!
//! Values are `Zeroizing`, so a password is wiped on drop.

use std::collections::HashMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

#[derive(Default)]
pub struct SecretVault {
    passwords: Mutex<HashMap<String, Zeroizing<String>>>,
}

impl SecretVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep a password for this host until the app exits.
    pub fn remember(&self, host_id: &str, password: &str) {
        if let Ok(mut passwords) = self.passwords.lock() {
            passwords.insert(host_id.to_string(), Zeroizing::new(password.to_string()));
        }
    }

    pub fn recall(&self, host_id: &str) -> Option<Zeroizing<String>> {
        self.passwords
            .lock()
            .ok()
            .and_then(|passwords| passwords.get(host_id).cloned())
    }

    pub fn has(&self, host_id: &str) -> bool {
        self.passwords
            .lock()
            .map(|passwords| passwords.contains_key(host_id))
            .unwrap_or(false)
    }

    /// Drop one host's password — used when the credential is rejected, so a
    /// stale password is not retried forever into an account lockout.
    pub fn forget(&self, host_id: &str) {
        if let Ok(mut passwords) = self.passwords.lock() {
            passwords.remove(host_id);
        }
    }

    pub fn forget_all(&self) {
        if let Ok(mut passwords) = self.passwords.lock() {
            passwords.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_recalls_and_forgets() {
        let vault = SecretVault::new();
        assert!(!vault.has("h-1"));
        assert!(vault.recall("h-1").is_none());

        vault.remember("h-1", "pass123");
        assert!(vault.has("h-1"));
        assert_eq!(vault.recall("h-1").unwrap().as_str(), "pass123");

        // Re-remembering replaces rather than stacking.
        vault.remember("h-1", "newpass");
        assert_eq!(vault.recall("h-1").unwrap().as_str(), "newpass");

        vault.forget("h-1");
        assert!(!vault.has("h-1"));
    }

    #[test]
    fn hosts_do_not_share_passwords() {
        let vault = SecretVault::new();
        vault.remember("h-1", "one");
        vault.remember("h-2", "two");

        assert_eq!(vault.recall("h-1").unwrap().as_str(), "one");
        assert_eq!(vault.recall("h-2").unwrap().as_str(), "two");

        vault.forget("h-1");
        assert!(!vault.has("h-1"));
        assert!(vault.has("h-2"), "forgetting one host must not clear another");

        vault.forget_all();
        assert!(!vault.has("h-2"));
    }
}
