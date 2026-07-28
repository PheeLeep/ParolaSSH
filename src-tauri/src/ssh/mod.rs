//! SSH key storage, inspection and security auditing.
//!
//! Kept behind narrow Tauri commands rather than a filesystem plugin: the
//! webview must never read a private key. Functions return metadata only, and
//! private key bytes are dropped as soon as parsing is done.
//!
//! Every entry point takes an explicit `ssh_dir: &Path`, so the audit can be
//! tested against fixture directories on any platform.

pub mod audit;
pub mod config;
pub mod generate;
pub mod keys;
pub mod known_hosts;
pub mod paths;
pub mod perms;
pub mod store;

use serde::{Serialize, Serializer};

/// Errors surfaced to the frontend.
///
/// Messages are written for a human reading a dialog. They deliberately avoid
/// echoing file contents - only paths the user already knows about.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("Could not locate your home directory.")]
    NoHomeDir,

    #[error("{0}")]
    Io(String),

    #[error("{0}")]
    Invalid(String),

    #[error("{0}")]
    Unsupported(String),
}

impl SshError {
    pub fn io(context: &str, error: std::io::Error) -> Self {
        Self::Io(format!("{context}: {error}"))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }
}

// Tauri requires command errors to be serializable; the display string is all
// the frontend needs.
impl Serialize for SshError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type SshResult<T> = Result<T, SshError>;
