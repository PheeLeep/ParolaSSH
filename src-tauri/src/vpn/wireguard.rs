//! Noticing plain WireGuard, which has no daemon to ask.
//!
//! Vanilla WireGuard is kernel interfaces plus a root-only `wg` tool, making
//! this the shallowest detector here: Linux can list interfaces without
//! privileges, macOS and Windows get presence detection only.
//!
//! Interfaces owned by the branded meshes (NetBird runs its kernel WireGuard as
//! `wt0`) are ignored — they have their own entry, and counting them twice
//! would fabricate a VPN conflict.

use super::{VpnKind, VpnStatus};

#[cfg(target_os = "linux")]
pub async fn status() -> VpnStatus {
    use super::{run_cli, CliOutcome};

    // `ip -j link show type wireguard` lists every WireGuard interface with
    // no privileges needed — unlike `wg show`, which wants CAP_NET_ADMIN.
    let interfaces = match run_cli("ip", &["-j", "link", "show", "type", "wireguard"]).await {
        CliOutcome::Ran { stdout, .. } => parse_interfaces(&stdout),
        _ => Vec::new(),
    };
    let tunnels = plain_tunnels(interfaces);

    if !tunnels.is_empty() {
        return VpnStatus {
            kind: VpnKind::Wireguard,
            installed: true,
            up: true,
            detail: format!("{} up", tunnels.join(", ")),
        };
    }

    // No tunnel up; installation comes down to the userspace tool's presence.
    for candidate in ["wg", "/usr/bin/wg"] {
        if let CliOutcome::Ran { .. } = run_cli(candidate, &["--version"]).await {
            return VpnStatus {
                kind: VpnKind::Wireguard,
                installed: true,
                up: false,
                detail: "no active tunnels".to_string(),
            };
        }
    }

    VpnStatus::not_installed(VpnKind::Wireguard)
}

#[cfg(target_os = "macos")]
pub async fn status() -> VpnStatus {
    use super::{run_cli, CliOutcome};

    let app_installed = std::path::Path::new("/Applications/WireGuard.app").exists();
    let tool_installed = ["/opt/homebrew/bin/wg", "/usr/local/bin/wg"]
        .iter()
        .any(|path| std::path::Path::new(path).exists());

    if !app_installed && !tool_installed {
        return VpnStatus::not_installed(VpnKind::Wireguard);
    }

    // The network extension process exists only while a tunnel is up. Tunnels
    // from the bare tool are anonymous `utun` devices and stay invisible here.
    let running = matches!(
        run_cli("/usr/bin/pgrep", &["-x", "WireGuardNetworkExtension"]).await,
        CliOutcome::Ran { success: true, .. }
    );

    VpnStatus {
        kind: VpnKind::Wireguard,
        installed: true,
        up: running,
        detail: if running {
            "tunnel active".to_string()
        } else {
            "no tunnel detected".to_string()
        },
    }
}

#[cfg(target_os = "windows")]
pub async fn status() -> VpnStatus {
    use super::{run_cli, CliOutcome};

    if !std::path::Path::new(r"C:\Program Files\WireGuard\wireguard.exe").exists() {
        return VpnStatus::not_installed(VpnKind::Wireguard);
    }

    // Tunnel services and the GUI are both `wireguard.exe`, so a match means
    // "running", not "tunnel up".
    let running = match run_cli("tasklist", &["/FI", "IMAGENAME eq wireguard.exe", "/NH"]).await {
        CliOutcome::Ran { stdout, .. } => stdout.contains("wireguard.exe"),
        _ => false,
    };

    VpnStatus {
        kind: VpnKind::Wireguard,
        installed: true,
        up: running,
        detail: if running {
            "running (tunnel state not visible)".to_string()
        } else {
            "not running".to_string()
        },
    }
}

/// Interface names out of `ip -j link` output; unparseable input yields an
/// empty list, not an error. `ifname` is optional because iproute2 emits an
/// empty `{}` per non-matching interface, which a strict parse would reject.
#[cfg(any(target_os = "linux", test))]
fn parse_interfaces(stdout: &str) -> Vec<String> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Link {
        ifname: Option<String>,
    }

    serde_json::from_str::<Vec<Link>>(stdout)
        .map(|links| links.into_iter().filter_map(|link| link.ifname).collect())
        .unwrap_or_default()
}

/// Drop interfaces owned by branded meshes. `wt*` is NetBird's kernel-mode
/// naming; a hand-written "wt-something" would be missed, which is the cheaper
/// mistake — a phantom second VPN alarms, a missed name only stays quiet.
#[cfg(any(target_os = "linux", test))]
fn plain_tunnels(interfaces: Vec<String>) -> Vec<String> {
    interfaces
        .into_iter()
        .filter(|name| !name.starts_with("wt"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ip_json_link_output() {
        let stdout = r#"[{"ifindex":5,"ifname":"wg0","flags":["POINTOPOINT","NOARP","UP","LOWER_UP"],"mtu":1420},{"ifindex":6,"ifname":"homelab","flags":["UP"],"mtu":1420}]"#;
        assert_eq!(parse_interfaces(stdout), vec!["wg0", "homelab"]);
    }

    #[test]
    fn empty_objects_from_the_type_filter_do_not_hide_a_match() {
        // With `show type wireguard`, iproute2 emits `{}` for every
        // non-matching interface; the real entry sits between them.
        let stdout = r#"[{},{},{"ifindex":5,"ifname":"wg0","mtu":1420},{}]"#;
        assert_eq!(parse_interfaces(stdout), vec!["wg0"]);
        // And a machine with no WireGuard at all is all-empty, not an error.
        assert!(parse_interfaces("[{},{},{},{},{},{}]").is_empty());
    }

    #[test]
    fn garbage_output_is_an_empty_list_not_an_error() {
        assert!(parse_interfaces("Device \"wireguard\" does not exist.\n").is_empty());
        assert!(parse_interfaces("").is_empty());
    }

    #[test]
    fn netbirds_kernel_interface_is_not_counted_as_plain_wireguard() {
        let tunnels = plain_tunnels(vec!["wt0".to_string(), "wg0".to_string()]);
        assert_eq!(tunnels, vec!["wg0"]);
    }
}
