//! Tauri commands for the local VPN picture.

use serde::Serialize;

use super::tailscale::{self, PeerListing};
use super::{bind, Freshness, VpnBinding, VpnStatus};

/// Everything the UI shows about VPNs, gathered in one detection pass.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnOverview {
    /// Every VPN this module recognises, installed or not.
    pub statuses: Vec<VpnStatus>,
    /// Only the hostnames that could be tied to a VPN appear here.
    pub bindings: Vec<VpnBinding>,
    /// What `twingate resources` reported - empty off Linux or with the
    /// service stopped.
    pub twingate_resources: Vec<ResourceInfo>,
}

/// One Twingate resource, shaped for display.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub name: String,
    pub address: String,
    pub alias: Option<String>,
    /// The client's own words, e.g. "Auth expires in 4 days".
    pub auth_status: String,
    /// Whether access would be refused right now, so the UI can flag it.
    pub needs_auth: bool,
}

/// The local VPN picture, plus which of the given addresses each VPN owns.
///
/// One command rather than two so the navbar badges and the host-row glyphs
/// share a single round of CLI calls per poll. Infallible by design: a
/// machine with no VPNs returns two `not installed` entries and no bindings,
/// not an error, so the UI renders the same shape either way.
///
/// `force` distinguishes the scheduled poll from the user pressing refresh:
/// a poll takes what the cache has, a person asking gets a real CLI round.
#[tauri::command]
pub async fn vpn_overview(hostnames: Vec<String>, force: bool) -> VpnOverview {
    let freshness = if force {
        Freshness::Forced
    } else {
        Freshness::Cached
    };

    let (statuses, resources) = tokio::join!(
        super::statuses(freshness),
        super::resources(freshness)
    );

    let bindings = hostnames
        .iter()
        .filter_map(|hostname| bind(hostname, &resources, &statuses))
        .collect();

    let twingate_resources = resources
        .into_iter()
        .map(|resource| ResourceInfo {
            needs_auth: resource.needs_auth(),
            name: resource.name,
            address: resource.address,
            alias: resource.alias,
            auth_status: resource.auth_status,
        })
        .collect();

    VpnOverview {
        statuses,
        bindings,
        twingate_resources,
    }
}

/// The tailnet's other machines, for importing as hosts.
///
/// Read-only, like the rest of the VPN module: it lists what Tailscale already
/// knows and never logs in, starts the daemon, or changes tailnet state.
#[tauri::command]
pub async fn tailscale_peers() -> PeerListing {
    tailscale::peers().await
}
