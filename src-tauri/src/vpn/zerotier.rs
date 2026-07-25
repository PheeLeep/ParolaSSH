//! Asking — with a fallback to noticing — the ZeroTier client.
//!
//! `zerotier-cli -j info` answers precisely, but it may only be run by
//! whoever can read the service's auth token — root, usually. When the CLI
//! is present but refuses us, the service process is the next best witness:
//! running is treated as up, with wording that keeps the uncertainty visible.
//! The same bargain the Twingate detector strikes on macOS and Windows.

use serde::Deserialize;

use super::{run_cli, CliOutcome, VpnKind, VpnStatus};

/// The one field of `zerotier-cli -j info` this module reads. `online`
/// means the node reaches ZeroTier's root servers — the closest thing the
/// client has to "connected".
#[derive(Deserialize)]
struct InfoJson {
    online: bool,
}

/// CLI candidates as (program, args). Windows has no standalone
/// `zerotier-cli` binary — the service executable doubles as the CLI
/// behind `-q` — and the .bat shim it ships cannot be spawned directly.
fn candidates() -> &'static [(&'static str, &'static [&'static str])] {
    #[cfg(target_os = "windows")]
    return &[
        (
            r"C:\ProgramData\ZeroTier\One\zerotier-one_x64.exe",
            &["-q", "-j", "info"],
        ),
    ];

    #[cfg(not(target_os = "windows"))]
    return &[
        ("zerotier-cli", &["-j", "info"]),
        ("/usr/sbin/zerotier-cli", &["-j", "info"]),
        ("/usr/local/bin/zerotier-cli", &["-j", "info"]),
    ];
}

pub async fn status() -> VpnStatus {
    for (program, args) in candidates() {
        match run_cli(program, args).await {
            CliOutcome::Missing => continue,
            CliOutcome::TimedOut => {
                return VpnStatus {
                    kind: VpnKind::Zerotier,
                    installed: true,
                    up: false,
                    detail: "the ZeroTier client is not responding".to_string(),
                }
            }
            CliOutcome::Ran { stdout, .. } => {
                if let Some(status) = interpret(&stdout) {
                    return status;
                }
                // The CLI exists but would not answer — almost always the
                // root-readable auth token. The process still tells us
                // whether ZeroTier is at least running.
                return presence().await;
            }
        }
    }

    VpnStatus::not_installed(VpnKind::Zerotier)
}

/// `None` when the output is not the info JSON — the caller falls back to
/// presence detection rather than guessing from an error message.
fn interpret(stdout: &str) -> Option<VpnStatus> {
    let parsed = serde_json::from_str::<InfoJson>(stdout).ok()?;

    Some(VpnStatus {
        kind: VpnKind::Zerotier,
        installed: true,
        up: parsed.online,
        detail: if parsed.online {
            "connected".to_string()
        } else {
            "offline".to_string()
        },
    })
}

/// The best an unprivileged caller can say: the service is or is not there.
async fn presence() -> VpnStatus {
    #[cfg(windows)]
    let running = match run_cli(
        "tasklist",
        &["/FI", "IMAGENAME eq zerotier-one_x64.exe", "/NH"],
    )
    .await
    {
        CliOutcome::Ran { stdout, .. } => stdout.contains("zerotier-one_x64.exe"),
        _ => false,
    };

    #[cfg(not(windows))]
    let running = matches!(
        run_cli("pgrep", &["-x", "zerotier-one"]).await,
        CliOutcome::Ran { success: true, .. }
    );

    VpnStatus {
        kind: VpnKind::Zerotier,
        installed: true,
        up: running,
        detail: if running {
            "service running (details need elevated access)".to_string()
        } else {
            "service not running".to_string()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_is_up() {
        let status = interpret(
            r#"{"address":"1122334455","clock":1753430000000,"online":true,"version":"1.14.2"}"#,
        )
        .unwrap();
        assert!(status.installed);
        assert!(status.up);
        assert_eq!(status.detail, "connected");
    }

    #[test]
    fn offline_is_down() {
        let status = interpret(r#"{"address":"1122334455","online":false,"version":"1.14.2"}"#).unwrap();
        assert!(!status.up);
        assert_eq!(status.detail, "offline");
    }

    #[test]
    fn a_refusal_is_not_interpreted_as_a_state() {
        // What the CLI prints without the auth token — the caller must fall
        // back to presence detection, not read this as "offline".
        assert!(interpret("missing authentication token and authtoken.secret not found").is_none());
        assert!(interpret("").is_none());
    }
}
