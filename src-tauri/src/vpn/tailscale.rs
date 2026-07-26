//! Asking the Tailscale client how it feels, and who else is on the tailnet.
//!
//! `tailscale status --json` is the one interface that behaves identically on
//! all three platforms, so it is the only one used. It also carries the peer
//! list, so importing tailnet machines as hosts needs no second source — the
//! daemon's local API socket would mean HTTP over a Unix socket on two
//! platforms and a named pipe on the third, for data already in hand.

use serde::{Deserialize, Serialize};

use super::{run_cli, CliOutcome, VpnKind, VpnStatus};

/// The fields of `tailscale status --json` this module reads.
#[derive(Deserialize)]
struct StatusJson {
    #[serde(rename = "BackendState")]
    backend_state: String,
    /// Keyed by node key. Absent when logged out.
    #[serde(rename = "Peer", default)]
    peer: std::collections::BTreeMap<String, PeerJson>,
}

#[derive(Deserialize)]
struct PeerJson {
    #[serde(rename = "HostName", default)]
    host_name: String,
    /// MagicDNS name, with a trailing dot: `box.tail1234.ts.net.`
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<String>,
    #[serde(rename = "OS", default)]
    os: String,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "Tags", default)]
    tags: Vec<String>,
}

/// One tailnet machine, shaped for the import list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscalePeer {
    /// Short name, used as the suggested label.
    pub host_name: String,
    /// What a saved host would connect to — see `preferred_address`.
    pub address: String,
    /// The MagicDNS name without its trailing dot, when there is one.
    pub dns_name: Option<String>,
    /// The 100.64/10 address, kept so the UI can show both.
    pub tailscale_ip: Option<String>,
    /// Tailscale's own word: `linux`, `windows`, `macOS`, `iOS`…
    pub os: String,
    pub online: bool,
    /// ACL tags with the `tag:` prefix stripped, for import as host tags.
    pub tags: Vec<String>,
}

/// What the peer list could tell us. Not a bare `Vec`: "logged out" and
/// "a tailnet of one" are different answers and the UI says so differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PeerListing {
    /// The CLI is not on this machine.
    NotInstalled,
    /// Installed, but the daemon is not in a state that knows any peers.
    Unavailable { detail: String },
    Peers { peers: Vec<TailscalePeer> },
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

/// List the other machines on this tailnet.
pub async fn peers() -> PeerListing {
    for candidate in candidates() {
        match run_cli(candidate, &["status", "--json"]).await {
            CliOutcome::Missing => continue,
            CliOutcome::TimedOut => {
                return PeerListing::Unavailable {
                    detail: "the Tailscale client is not responding".to_string(),
                }
            }
            CliOutcome::Ran { stdout, .. } => return interpret_peers(&stdout),
        }
    }

    PeerListing::NotInstalled
}

/// Turn the status JSON into a peer listing. Pure, so the shapes are tested
/// without Tailscale installed.
fn interpret_peers(stdout: &str) -> PeerListing {
    let Ok(status) = serde_json::from_str::<StatusJson>(stdout) else {
        return PeerListing::Unavailable {
            detail: "the Tailscale service is not running".to_string(),
        };
    };

    // Peers are only meaningful once the backend is up; logged out, the map is
    // empty and "no machines" would read as an empty tailnet rather than a
    // login prompt.
    if status.backend_state != "Running" {
        return PeerListing::Unavailable {
            detail: match status.backend_state.as_str() {
                "NeedsLogin" => "Tailscale needs login".to_string(),
                "Stopped" => "Tailscale is stopped".to_string(),
                other => format!("Tailscale is {other}"),
            },
        };
    }

    let mut peers: Vec<TailscalePeer> = status.peer.into_values().map(peer_from).collect();
    // Online first, then by name: the machines you can reach now are the ones
    // you are here to add.
    peers.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.host_name.to_lowercase().cmp(&b.host_name.to_lowercase()))
    });

    PeerListing::Peers { peers }
}

fn peer_from(peer: PeerJson) -> TailscalePeer {
    let dns_name = peer
        .dns_name
        .trim_end_matches('.')
        .trim()
        .to_string();
    let dns_name = (!dns_name.is_empty()).then_some(dns_name);

    // IPv4 first: the 100.64/10 address is the one a user recognises, and some
    // hosts carry only an IPv6.
    let tailscale_ip = peer
        .tailscale_ips
        .iter()
        .find(|ip| ip.contains('.'))
        .or_else(|| peer.tailscale_ips.first())
        .cloned();

    TailscalePeer {
        address: preferred_address(dns_name.as_deref(), tailscale_ip.as_deref(), &peer.host_name),
        host_name: peer.host_name,
        dns_name,
        tailscale_ip,
        os: peer.os,
        online: peer.online,
        tags: peer
            .tags
            .iter()
            .map(|tag| tag.trim_start_matches("tag:").to_string())
            .filter(|tag| !tag.is_empty())
            .collect(),
    }
}

/// What a saved host should connect to.
///
/// MagicDNS wins: it survives the node changing address, which the 100.x one
/// does not. The IP is the fallback for tailnets with MagicDNS off, and the
/// bare hostname the last resort — better than saving a host with no address.
fn preferred_address(dns_name: Option<&str>, ip: Option<&str>, host_name: &str) -> String {
    dns_name
        .or(ip)
        .unwrap_or(host_name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real `tailscale status --json`, two peers and a `Self`
    /// that must not appear in the list.
    const STATUS: &str = r#"{
      "BackendState": "Running",
      "Self": { "HostName": "my-laptop", "DNSName": "my-laptop.tail1234.ts.net.",
                "TailscaleIPs": ["100.64.0.1"], "OS": "linux", "Online": true },
      "Peer": {
        "nodekey:aaa": { "HostName": "web-01", "DNSName": "web-01.tail1234.ts.net.",
                         "TailscaleIPs": ["100.64.0.9", "fd7a:115c::9"],
                         "OS": "linux", "Online": true, "Tags": ["tag:server", "tag:prod"] },
        "nodekey:bbb": { "HostName": "old-nas", "DNSName": "",
                         "TailscaleIPs": ["100.64.0.4"], "OS": "linux", "Online": false }
      }
    }"#;

    fn peers_of(json: &str) -> Vec<TailscalePeer> {
        match interpret_peers(json) {
            PeerListing::Peers { peers } => peers,
            other => panic!("expected peers, got {other:?}"),
        }
    }

    #[test]
    fn peers_are_listed_online_first_without_self() {
        let peers = peers_of(STATUS);
        assert_eq!(peers.len(), 2, "`Self` is this machine, not a peer");

        assert_eq!(peers[0].host_name, "web-01");
        assert!(peers[0].online);
        assert_eq!(peers[1].host_name, "old-nas");
        assert!(!peers[1].online);
    }

    #[test]
    fn magicdns_is_preferred_over_the_hundred_address() {
        let peers = peers_of(STATUS);

        // The stable name, with its trailing dot removed.
        assert_eq!(peers[0].address, "web-01.tail1234.ts.net");
        assert_eq!(peers[0].dns_name.as_deref(), Some("web-01.tail1234.ts.net"));
        // IPv4 is picked out of a list that also holds an IPv6.
        assert_eq!(peers[0].tailscale_ip.as_deref(), Some("100.64.0.9"));

        // With MagicDNS off the address falls back to the IP.
        assert_eq!(peers[1].address, "100.64.0.4");
        assert_eq!(peers[1].dns_name, None);
    }

    #[test]
    fn acl_tags_lose_their_prefix_for_import() {
        let peers = peers_of(STATUS);
        assert_eq!(peers[0].tags, vec!["server", "prod"]);
        assert!(peers[1].tags.is_empty());
    }

    #[test]
    fn a_logged_out_client_is_not_an_empty_tailnet() {
        let listing = interpret_peers(r#"{"BackendState":"NeedsLogin","Peer":{}}"#);
        assert_eq!(
            listing,
            PeerListing::Unavailable {
                detail: "Tailscale needs login".to_string()
            }
        );

        // Neither is a daemon that answered with prose instead of JSON.
        assert!(matches!(
            interpret_peers("failed to connect to local tailscaled"),
            PeerListing::Unavailable { .. }
        ));
    }

    #[test]
    fn a_running_tailnet_of_one_is_an_empty_list() {
        let listing = interpret_peers(r#"{"BackendState":"Running","Peer":{}}"#);
        assert_eq!(listing, PeerListing::Peers { peers: Vec::new() });
    }

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
