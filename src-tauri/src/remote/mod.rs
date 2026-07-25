//! Talking to a remote machine.
//!
//! Split from `ssh` (which is about *local* key files) because the trust model
//! is different: everything here crosses a network to a host that may not be
//! what it claims. Host keys are checked against `~/.ssh/known_hosts` before a
//! password is ever offered, and credentials live in memory for the lifetime
//! of the session and no longer.
//!
//! The layering is deliberate:
//!   * `client`   — connect and authenticate; owns the host key decision.
//!   * `registry` — keeps authenticated sessions alive between commands.
//!   * `power`    — builds shutdown/reboot commands per remote OS.
//!   * `shell`    — interactive PTY, streamed to the webview as events.
//!   * `stream`   — long-running command output, streamed without a PTY.
//!   * `services` — list, act on, and read logs of system services.
//!   * `metrics`  — one-round-trip performance sampling.
//!   * `updates`  — read-only pending-update queries per package manager.
//!   * `audit`    — remote security posture, tiered by cost.
//!   * `probe`    — is the port even open, before we try anything else.

pub mod audit;
pub mod client;
pub mod commands;
pub mod metrics;
pub mod power;
pub mod probe;
pub mod registry;
pub mod secrets;
pub mod services;
pub mod shell;
pub mod stream;
pub mod updates;

use serde::Serialize;

/// The remote operating system family, which decides how to phrase a command.
///
/// `Unknown` is a real outcome, not a failure: a locked-down shell may refuse
/// both probes, and the UI should say so rather than guess and reboot the
/// wrong way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OsFamily {
    Linux,
    Macos,
    Bsd,
    Windows,
    Unknown,
}

impl OsFamily {
    pub fn is_unix(self) -> bool {
        matches!(self, Self::Linux | Self::Macos | Self::Bsd)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Macos => "macOS",
            Self::Bsd => "BSD",
            Self::Windows => "Windows",
            Self::Unknown => "Unknown",
        }
    }
}

/// What a remote command produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
}

impl CommandOutput {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// stderr if there is any, otherwise stdout — whichever explains a failure.
    pub fn failure_text(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_string();
        }
        let stdout = self.stdout.trim();
        if !stdout.is_empty() {
            return stdout.to_string();
        }
        match self.exit_code {
            Some(code) => format!("The command exited with status {code}."),
            None => "The command was terminated without an exit status.".to_string(),
        }
    }
}
