//! Which VPN clients live on this machine, and are they up?
//!
//! ParolaSSH does not tunnel anything itself — these clients sit at the OS
//! network layer, so traffic already flows through them. Knowing about them
//! only buys a better diagnosis than "no response from 100.x.y.z".
//!
//! Detection is shallow and read-only: find the client, ask its own CLI, never
//! touch its configuration. A VPN that is not installed is a normal outcome.

mod cache;
pub mod commands;
mod netbird;
mod tailscale;
mod twingate;
mod wireguard;
mod zerotier;

use std::net::Ipv4Addr;
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;

pub use cache::Freshness;
use cache::TtlCache;

/// How long a client's CLI gets before we assume it is wedged. These are local
/// commands that normally return in milliseconds.
const CLI_TIMEOUT: Duration = Duration::from_secs(3);

/// The last detection pass, shared by the navbar poll, the host-row glyphs and
/// every probe explanation. See `cache` for why this exists.
static STATUS_CACHE: TtlCache<Vec<VpnStatus>> = TtlCache::new(cache::STATUS_TTL);

/// The last `twingate resources` listing, on a much longer TTL.
static RESOURCE_CACHE: TtlCache<Vec<twingate::TwingateResource>> =
    TtlCache::new(cache::RESOURCE_TTL);

/// Every VPN's condition, from the cache unless the caller insists otherwise.
/// **Prefer this to `detect_all`** — that one always spawns processes.
pub async fn statuses(freshness: Freshness) -> Vec<VpnStatus> {
    STATUS_CACHE.get(freshness, detect_all).await
}

/// The Twingate resource list, from the cache unless the caller insists.
pub async fn resources(freshness: Freshness) -> Vec<twingate::TwingateResource> {
    RESOURCE_CACHE.get(freshness, twingate::resources).await
}

/// The VPNs this module knows how to recognise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VpnKind {
    Tailscale,
    Twingate,
    Netbird,
    Zerotier,
    /// Plain WireGuard tunnels, with the branded meshes' interfaces
    /// excluded — see `wireguard`.
    Wireguard,
}

impl VpnKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tailscale => "Tailscale",
            Self::Twingate => "Twingate",
            Self::Netbird => "NetBird",
            Self::Zerotier => "ZeroTier",
            Self::Wireguard => "WireGuard",
        }
    }

    /// Whether this VPN assigns out of 100.64.0.0/10, so a bare CGNAT address
    /// may be attributed to it. ZeroTier uses RFC1918 and WireGuard whatever
    /// the config says.
    pub fn uses_cgnat(self) -> bool {
        matches!(self, Self::Tailscale | Self::Twingate | Self::Netbird)
    }
}

/// One VPN client's condition on the local machine.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatus {
    pub kind: VpnKind,
    /// Whether the client software exists on this machine at all.
    pub installed: bool,
    /// Whether it currently provides connectivity. Meaningful only when
    /// `installed`, and best-effort where the client has no status command.
    pub up: bool,
    /// Plain-language state, ready to show: "connected", "needs login", …
    pub detail: String,
}

impl VpnStatus {
    fn not_installed(kind: VpnKind) -> Self {
        Self {
            kind,
            installed: false,
            up: false,
            detail: "not installed".to_string(),
        }
    }
}

/// Check every VPN we know how to recognise, concurrently. Boxed so adding a
/// provider is one line here plus its module.
///
/// Spawns one process per provider every time it is called; `statuses` is the
/// door most callers should use.
async fn detect_all() -> Vec<VpnStatus> {
    let checks: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = VpnStatus> + Send>>> = vec![
        Box::pin(tailscale::status()),
        Box::pin(twingate::status()),
        Box::pin(netbird::status()),
        Box::pin(zerotier::status()),
        Box::pin(wireguard::status()),
    ];
    futures_util::future::join_all(checks).await
}

/// What a saved address suggests about the network it lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressHint {
    /// A `*.ts.net` MagicDNS name — unambiguously Tailscale.
    Tailscale,
    /// An IP in 100.64.0.0/10, which Tailscale, Twingate and NetBird all assign
    /// from — and which is rarely a genuine ISP address. A hint, not a verdict.
    CarrierGradeNat,
}

/// Read what the address gives away. Purely lexical — resolving DNS would be
/// useless here, since these names only resolve while the VPN is up.
pub fn address_hint(hostname: &str) -> Option<AddressHint> {
    let host = hostname.trim().trim_end_matches('.');

    if host.to_ascii_lowercase().ends_with(".ts.net") {
        return Some(AddressHint::Tailscale);
    }

    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let [first, second, _, _] = ip.octets();
        if first == 100 && (64..=127).contains(&second) {
            return Some(AddressHint::CarrierGradeNat);
        }
    }

    None
}

/// One saved address's relationship to a VPN, for the UI to show as a glyph.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnBinding {
    pub hostname: String,
    /// Which VPN, when it can be told; `None` when several could own the range.
    pub kind: Option<VpnKind>,
    /// Ready to show, e.g. "Twingate resource 'acme project'".
    pub description: String,
}

/// Tie one address to the VPN that carries it, if any.
///
/// Twingate's resource list outranks the lexical hints — it is the client
/// stating ownership. A bare CGNAT address is attributed only when exactly one
/// CGNAT VPN is installed; otherwise the glyph would be a guess.
pub fn bind(
    hostname: &str,
    resources: &[twingate::TwingateResource],
    statuses: &[VpnStatus],
) -> Option<VpnBinding> {
    if let Some(resource) = resources.iter().find(|r| r.matches(hostname)) {
        return Some(VpnBinding {
            hostname: hostname.to_string(),
            kind: Some(VpnKind::Twingate),
            description: format!("Twingate resource '{}'", resource.name),
        });
    }

    match address_hint(hostname)? {
        AddressHint::Tailscale => Some(VpnBinding {
            hostname: hostname.to_string(),
            kind: Some(VpnKind::Tailscale),
            description: "Tailscale address".to_string(),
        }),
        AddressHint::CarrierGradeNat => {
            // Only CGNAT-assigning VPNs are candidates; a WireGuard alongside
            // them must not widen the ambiguity.
            let installed: Vec<&VpnStatus> = statuses
                .iter()
                .filter(|status| status.installed && status.kind.uses_cgnat())
                .collect();
            match installed.as_slice() {
                [] => None,
                [only] => Some(VpnBinding {
                    hostname: hostname.to_string(),
                    kind: Some(only.kind),
                    description: format!("In {}'s address range", only.kind.label()),
                }),
                several => {
                    let names = several
                        .iter()
                        .map(|status| status.kind.label())
                        .collect::<Vec<_>>()
                        .join(" or ");
                    Some(VpnBinding {
                        hostname: hostname.to_string(),
                        kind: None,
                        description: format!("In a VPN address range ({names})"),
                    })
                }
            }
        }
    }
}

/// The extra sentence a failed probe earns when its address looks VPN-bound.
/// `None` when there is nothing useful to add.
///
/// Reads the cache throughout: a page of unreachable hosts explains itself from
/// one detection pass, and the answer is at most `STATUS_TTL` old — which is
/// fresher than the probe that just failed.
pub async fn explain_unreachable(hostname: &str) -> Option<String> {
    // Resources are often plain LAN addresses no heuristic could attribute, so
    // the list outranks the lexical hints.
    let resources = resources(Freshness::Cached).await;
    let statuses = statuses(Freshness::Cached).await;

    if let Some(resource) = resources.iter().find(|r| r.matches(hostname)) {
        let twingate = statuses.iter().find(|s| s.kind == VpnKind::Twingate)?;
        return Some(twingate_advice(resource, twingate));
    }

    advice(address_hint(hostname)?, &statuses)
}

/// What to say when the dead address is a known Twingate resource. Every branch
/// hedges on "unless you are on the host's own network", because a resource may
/// be reachable directly on premise and this module cannot tell which side of
/// the wall it is on.
fn twingate_advice(resource: &twingate::TwingateResource, twingate: &VpnStatus) -> String {
    if !twingate.up {
        format!(
            "This address is the Twingate resource '{}', and Twingate is not connected \
             on this machine ({}). Unless this computer is on the host's own network \
             right now, start Twingate and try again.",
            resource.name, twingate.detail
        )
    } else if resource.needs_auth() {
        // Name the exact command: `ssh` to a resource needing re-auth just
        // hangs, with no error to work from.
        format!(
            "This address is the Twingate resource '{}', which needs authentication \
             ({}). Run `twingate auth \"{}\"` or approve the Twingate notification, \
             then try again.",
            resource.name, resource.auth_status, resource.name
        )
    } else {
        format!(
            "This address is the Twingate resource '{}', and Twingate is connected \
             with valid authentication — the host itself may be off, or not \
             reachable from the network this computer is on.",
            resource.name
        )
    }
}

/// Turn a hint plus the local VPN picture into advice, or stay quiet. Split
/// from `explain_unreachable` so the wording is testable without a VPN client.
fn advice(hint: AddressHint, statuses: &[VpnStatus]) -> Option<String> {
    match hint {
        AddressHint::Tailscale => {
            let tailscale = statuses.iter().find(|s| s.kind == VpnKind::Tailscale)?;
            if !tailscale.installed {
                Some(
                    "This is a Tailscale address (*.ts.net), but Tailscale does not appear \
                     to be installed on this machine."
                        .to_string(),
                )
            } else if !tailscale.up {
                Some(format!(
                    "This is a Tailscale address and Tailscale is not connected on this \
                     machine ({}). Reconnect Tailscale and try again.",
                    tailscale.detail
                ))
            } else {
                Some(
                    "Tailscale is connected on this machine, so the host itself is \
                     likely off or no longer on the tailnet."
                        .to_string(),
                )
            }
        }

        AddressHint::CarrierGradeNat => {
            let down: Vec<&VpnStatus> = statuses
                .iter()
                .filter(|status| status.installed && !status.up && status.kind.uses_cgnat())
                .collect();

            match down.as_slice() {
                // Nothing down to blame: with no CGNAT VPN here, 100.x may be a
                // genuine carrier address, and if all are up the base message
                // already covers it.
                [] => None,
                [only] => Some(format!(
                    "This address is in the range VPNs like Tailscale and Twingate use, \
                     and {} is not connected on this machine ({}). If this host is \
                     reached through it, reconnect and try again.",
                    only.kind.label(),
                    only.detail
                )),
                several => {
                    let names = several
                        .iter()
                        .map(|status| status.kind.label())
                        .collect::<Vec<_>>()
                        .join(" and ");
                    Some(format!(
                        "This address is in the range VPNs like Tailscale and Twingate \
                         use, and neither {names} is connected on this machine. \
                         Reconnect the one this host belongs to and try again."
                    ))
                }
            }
        }
    }
}

/// What happened when we tried a client's CLI.
pub(crate) enum CliOutcome {
    /// Not there, or not runnable — either way, not installed.
    Missing,
    /// It ran but did not answer within `CLI_TIMEOUT`.
    TimedOut,
    /// It ran to completion, successfully or not.
    Ran {
        /// Read by the `pgrep`-based presence checks; Windows answers from
        /// stdout instead, because `tasklist` always exits zero.
        #[cfg_attr(windows, allow(dead_code))]
        success: bool,
        stdout: String,
    },
}

/// Run one VPN client CLI with a timeout and no console window. A missing
/// binary and one we may not execute both collapse to `Missing`.
pub(crate) async fn run_cli(program: &str, args: &[&str]) -> CliOutcome {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // The timeout drops the future; the child must die with it.
        .kill_on_drop(true);

    // Without this, every status check on Windows flashes a console window.
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    match tokio::time::timeout(CLI_TIMEOUT, command.output()).await {
        Err(_) => CliOutcome::TimedOut,
        Ok(Err(_)) => CliOutcome::Missing,
        Ok(Ok(output)) => CliOutcome::Ran {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(kind: VpnKind, installed: bool, up: bool, detail: &str) -> VpnStatus {
        VpnStatus {
            kind,
            installed,
            up,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn magicdns_names_hint_tailscale() {
        assert_eq!(address_hint("myhost.tail1234.ts.net"), Some(AddressHint::Tailscale));
        // Case and a trailing FQDN dot must not hide the suffix.
        assert_eq!(address_hint(" MyHost.Tail1234.TS.NET. "), Some(AddressHint::Tailscale));
    }

    #[test]
    fn cgnat_range_is_edge_exact() {
        assert_eq!(address_hint("100.64.0.0"), Some(AddressHint::CarrierGradeNat));
        assert_eq!(address_hint("100.127.255.255"), Some(AddressHint::CarrierGradeNat));
        // One step outside either edge of 100.64.0.0/10.
        assert_eq!(address_hint("100.63.255.255"), None);
        assert_eq!(address_hint("100.128.0.0"), None);
    }

    #[test]
    fn ordinary_addresses_hint_nothing() {
        assert_eq!(address_hint("192.168.1.10"), None);
        assert_eq!(address_hint("example.com"), None);
        // "ts.net" inside a name, not as its suffix, means nothing.
        assert_eq!(address_hint("ts.net.example.com"), None);
    }

    #[test]
    fn tailscale_down_names_the_fix() {
        let advice = advice(
            AddressHint::Tailscale,
            &[status(VpnKind::Tailscale, true, false, "needs login")],
        )
        .unwrap();
        assert!(advice.contains("not connected"));
        assert!(advice.contains("needs login"));
    }

    #[test]
    fn tailscale_up_blames_the_host_instead() {
        let advice = advice(
            AddressHint::Tailscale,
            &[status(VpnKind::Tailscale, true, true, "connected")],
        )
        .unwrap();
        assert!(advice.contains("host itself"));
    }

    #[test]
    fn cgnat_with_no_vpn_installed_stays_quiet() {
        // A bare 100.x address on a VPN-free machine may be a genuine
        // carrier address; guessing would mislead.
        let result = advice(
            AddressHint::CarrierGradeNat,
            &[
                status(VpnKind::Tailscale, false, false, "not installed"),
                status(VpnKind::Twingate, false, false, "not installed"),
            ],
        );
        assert!(result.is_none());
    }

    #[test]
    fn cgnat_names_the_one_vpn_that_is_down() {
        let advice = advice(
            AddressHint::CarrierGradeNat,
            &[
                status(VpnKind::Tailscale, true, true, "connected"),
                status(VpnKind::Twingate, true, false, "stopped"),
            ],
        )
        .unwrap();
        assert!(advice.contains("Twingate"));
        assert!(!advice.contains("Tailscale is not connected"));
    }

    fn resource(name: &str, address: &str, auth_status: &str) -> twingate::TwingateResource {
        twingate::TwingateResource {
            name: name.to_string(),
            address: address.to_string(),
            alias: None,
            auth_status: auth_status.to_string(),
        }
    }

    #[test]
    fn binding_prefers_the_resource_list_over_hints() {
        let resources = [resource("acme project", "192.168.9.0/24", "Auth expires in 4 days")];
        let statuses = [
            status(VpnKind::Tailscale, true, true, "connected"),
            status(VpnKind::Twingate, true, true, "connected"),
        ];

        let binding = bind("192.168.9.59", &resources, &statuses).unwrap();
        assert_eq!(binding.kind, Some(VpnKind::Twingate));
        assert!(binding.description.contains("acme project"));

        // A LAN address outside every resource stays unadorned.
        assert!(bind("192.168.50.1", &resources, &statuses).is_none());
    }

    #[test]
    fn binding_attributes_cgnat_only_when_it_is_unambiguous() {
        let one_vpn = [
            status(VpnKind::Tailscale, true, false, "stopped"),
            status(VpnKind::Twingate, false, false, "not installed"),
        ];
        let binding = bind("100.100.1.1", &[], &one_vpn).unwrap();
        assert_eq!(binding.kind, Some(VpnKind::Tailscale));

        // With both installed the range could belong to either; the glyph
        // must admit that instead of picking one.
        let both = [
            status(VpnKind::Tailscale, true, true, "connected"),
            status(VpnKind::Twingate, true, true, "connected"),
        ];
        assert_eq!(bind("100.100.1.1", &[], &both).unwrap().kind, None);

        // And with neither, a 100.x address is probably just a carrier IP.
        let neither = [
            status(VpnKind::Tailscale, false, false, "not installed"),
            status(VpnKind::Twingate, false, false, "not installed"),
        ];
        assert!(bind("100.100.1.1", &[], &neither).is_none());
    }

    #[test]
    fn only_cgnat_users_take_part_in_cgnat_attribution() {
        // WireGuard installed alongside NetBird must not widen the
        // ambiguity: NetBird is the only CGNAT candidate, so the address
        // is attributed to it outright.
        let statuses = [
            status(VpnKind::Netbird, true, true, "connected"),
            status(VpnKind::Wireguard, true, true, "wg0 up"),
        ];
        let binding = bind("100.100.1.1", &[], &statuses).unwrap();
        assert_eq!(binding.kind, Some(VpnKind::Netbird));

        // A machine with only non-CGNAT VPNs stays quiet about 100.x.
        let non_cgnat = [
            status(VpnKind::Zerotier, true, true, "connected"),
            status(VpnKind::Wireguard, true, false, "no active tunnels"),
        ];
        assert!(bind("100.100.1.1", &[], &non_cgnat).is_none());

        // And the same rule holds for the probe advice: a downed WireGuard
        // must not be blamed for a CGNAT address.
        let wireguard_down = [status(VpnKind::Wireguard, true, false, "no active tunnels")];
        assert!(advice(AddressHint::CarrierGradeNat, &wireguard_down).is_none());
    }

    #[test]
    fn ambiguous_cgnat_names_every_candidate() {
        let three = [
            status(VpnKind::Tailscale, true, true, "connected"),
            status(VpnKind::Twingate, true, true, "connected"),
            status(VpnKind::Netbird, true, true, "connected"),
        ];
        let binding = bind("100.100.1.1", &[], &three).unwrap();
        assert_eq!(binding.kind, None);
        assert!(binding.description.contains("Tailscale or Twingate or NetBird"));
    }

    #[test]
    fn a_resource_with_twingate_down_says_to_start_it_with_a_hedge() {
        let advice = twingate_advice(
            &resource("acme project", "192.168.1.0/24", "Auth expires in 4 days"),
            &status(VpnKind::Twingate, true, false, "not running"),
        );
        assert!(advice.contains("acme project"));
        assert!(advice.contains("start Twingate"));
        // The same LAN address may work directly on premise; the advice
        // must leave room for that instead of insisting on the VPN.
        assert!(advice.contains("own network"));
    }

    #[test]
    fn a_resource_needing_auth_names_the_exact_command() {
        let advice = twingate_advice(
            &resource("acme project", "192.168.1.0/24", "Auth required"),
            &status(VpnKind::Twingate, true, true, "connected"),
        );
        assert!(advice.contains(r#"twingate auth "acme project""#));
    }

    #[test]
    fn a_healthy_resource_blames_the_host_or_the_network() {
        let advice = twingate_advice(
            &resource("acme-git", "192.168.9.75", "Auth expires in 4 days"),
            &status(VpnKind::Twingate, true, true, "connected"),
        );
        assert!(advice.contains("host itself may be off"));
    }

    #[test]
    fn cgnat_with_both_down_names_both() {
        let advice = advice(
            AddressHint::CarrierGradeNat,
            &[
                status(VpnKind::Tailscale, true, false, "stopped"),
                status(VpnKind::Twingate, true, false, "not running"),
            ],
        )
        .unwrap();
        assert!(advice.contains("Tailscale and Twingate"));
    }
}
