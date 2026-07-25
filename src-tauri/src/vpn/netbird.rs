//! Asking the NetBird client how it feels.
//!
//! NetBird is the same shape of product as Tailscale — a WireGuard mesh
//! behind a coordination server — and offers the same kind of interface:
//! one `netbird status --json` that behaves identically on all three
//! platforms, so it is the only one used.

use serde::Deserialize;

use super::{run_cli, CliOutcome, VpnKind, VpnStatus};

/// The slice of `netbird status --json` this module reads.
#[derive(Deserialize)]
struct StatusJson {
    management: Option<Management>,
}

#[derive(Deserialize)]
struct Management {
    connected: bool,
}

/// Where the CLI might be, tried in order. The bare name covers a healthy
/// PATH; the absolute paths cover installs that do not touch it.
fn candidates() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    return &[
        "netbird",
        "/usr/local/bin/netbird",
        "/opt/homebrew/bin/netbird",
    ];

    #[cfg(target_os = "windows")]
    return &["netbird", r"C:\Program Files\NetBird\netbird.exe"];

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    return &["netbird", "/usr/bin/netbird"];
}

pub async fn status() -> VpnStatus {
    for candidate in candidates() {
        match run_cli(candidate, &["status", "--json"]).await {
            CliOutcome::Missing => continue,
            CliOutcome::TimedOut => {
                return VpnStatus {
                    kind: VpnKind::Netbird,
                    installed: true,
                    up: false,
                    detail: "the NetBird client is not responding".to_string(),
                }
            }
            CliOutcome::Ran { stdout, .. } => return interpret(&stdout),
        }
    }

    VpnStatus::not_installed(VpnKind::Netbird)
}

/// The management link is the health signal: connected peers can honestly
/// be zero on a quiet network, but a client that cannot reach its
/// management server is not going to route anything new. No JSON at all
/// means the CLI could not reach its own daemon.
fn interpret(stdout: &str) -> VpnStatus {
    let Ok(parsed) = serde_json::from_str::<StatusJson>(stdout) else {
        return VpnStatus {
            kind: VpnKind::Netbird,
            installed: true,
            up: false,
            detail: "the NetBird service is not running".to_string(),
        };
    };

    let (up, detail) = match parsed.management {
        Some(Management { connected: true }) => (true, "connected".to_string()),
        Some(_) => (false, "management disconnected — try `netbird up`".to_string()),
        None => (false, "not connected".to_string()),
    };

    VpnStatus {
        kind: VpnKind::Netbird,
        installed: true,
        up,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connected_management_link_is_up() {
        let status = interpret(
            r#"{"peers":{"total":3,"connected":2},"management":{"url":"https://api.netbird.io:443","connected":true},"netbirdIp":"100.92.1.2/16"}"#,
        );
        assert!(status.installed);
        assert!(status.up);
        assert_eq!(status.detail, "connected");
    }

    #[test]
    fn a_lost_management_link_is_down_with_the_fix_named() {
        let status = interpret(r#"{"management":{"url":"https://api.netbird.io:443","connected":false}}"#);
        assert!(!status.up);
        assert!(status.detail.contains("netbird up"));
    }

    #[test]
    fn no_json_means_the_daemon_is_gone() {
        let status = interpret("Error: Unable to connect to the daemon\n");
        assert!(status.installed);
        assert!(!status.up);
        assert!(status.detail.contains("service"));
    }
}
