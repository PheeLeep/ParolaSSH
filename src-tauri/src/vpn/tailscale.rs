//! Asking the Tailscale client how it feels.
//!
//! `tailscale status --json` is the one interface that behaves identically on
//! all three platforms, so it is the only one used. The daemon's local API
//! socket is richer — it can list peers, which the host-import feature will
//! eventually want — but it means speaking HTTP over a Unix socket on two
//! platforms and a named pipe on the third. Not worth it to learn one enum.

use serde::Deserialize;

use super::{run_cli, CliOutcome, VpnKind, VpnStatus};

/// The one field of `tailscale status --json` this module reads.
#[derive(Deserialize)]
struct StatusJson {
    #[serde(rename = "BackendState")]
    backend_state: String,
}

/// Where the CLI might be, tried in order.
///
/// The bare name covers a healthy PATH everywhere. The absolute paths cover
/// the installs that do not touch PATH: the macOS app bundle (whose GUI
/// binary answers CLI subcommands), the default Windows installer location,
/// and a Linux daemon-only install where sbin is not on a user PATH.
fn candidates() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    return &[
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ];

    #[cfg(target_os = "windows")]
    return &["tailscale", r"C:\Program Files\Tailscale\tailscale.exe"];

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    return &["tailscale", "/usr/bin/tailscale", "/usr/sbin/tailscale"];
}

pub async fn status() -> VpnStatus {
    for candidate in candidates() {
        match run_cli(candidate, &["status", "--json"]).await {
            // Not at this path; maybe at the next.
            CliOutcome::Missing => continue,
            CliOutcome::TimedOut => {
                return VpnStatus {
                    kind: VpnKind::Tailscale,
                    installed: true,
                    up: false,
                    detail: "the Tailscale client is not responding".to_string(),
                }
            }
            CliOutcome::Ran { stdout, .. } => return interpret(&stdout),
        }
    }

    VpnStatus::not_installed(VpnKind::Tailscale)
}

/// Read the backend state out of whatever the CLI printed.
///
/// The exit code is deliberately ignored: `tailscale status` exits non-zero
/// for states like `Stopped` that are perfectly good answers, so the JSON is
/// the only signal trusted. No JSON at all means the CLI could not reach its
/// own daemon — the service side of Tailscale is not running.
fn interpret(stdout: &str) -> VpnStatus {
    let Some(state) = serde_json::from_str::<StatusJson>(stdout)
        .ok()
        .map(|status| status.backend_state)
    else {
        return VpnStatus {
            kind: VpnKind::Tailscale,
            installed: true,
            up: false,
            detail: "the Tailscale service is not running".to_string(),
        };
    };

    let (up, detail) = match state.as_str() {
        "Running" => (true, "connected".to_string()),
        "Stopped" => (false, "stopped".to_string()),
        "NeedsLogin" => (false, "needs login".to_string()),
        "NeedsMachineAuth" => (false, "awaiting device approval".to_string()),
        "Starting" => (false, "still starting".to_string()),
        // A state this module has not heard of is shown as-is rather than
        // hidden — "InUseOtherUser" says more than "unknown" would.
        other => (false, other.to_string()),
    };

    VpnStatus {
        kind: VpnKind::Tailscale,
        installed: true,
        up,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_state_is_up() {
        let status = interpret(r#"{"BackendState":"Running","Version":"1.86.2"}"#);
        assert!(status.installed);
        assert!(status.up);
        assert_eq!(status.detail, "connected");
    }

    #[test]
    fn needs_login_is_down_with_a_reason() {
        let status = interpret(r#"{"BackendState":"NeedsLogin"}"#);
        assert!(status.installed);
        assert!(!status.up);
        assert_eq!(status.detail, "needs login");
    }

    #[test]
    fn no_json_means_the_daemon_is_gone() {
        // What the CLI prints when tailscaled is not running is prose, not
        // JSON — that shape is itself the answer.
        let status = interpret("failed to connect to local tailscaled; it doesn't appear to be running\n");
        assert!(status.installed);
        assert!(!status.up);
        assert!(status.detail.contains("service"));
    }

    #[test]
    fn unknown_states_are_shown_not_hidden() {
        let status = interpret(r#"{"BackendState":"InUseOtherUser"}"#);
        assert!(!status.up);
        assert_eq!(status.detail, "InUseOtherUser");
    }
}
