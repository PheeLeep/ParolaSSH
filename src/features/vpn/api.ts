/** The Rust command surface for the local VPN picture.
 *
 *  Read-only by design: the app reports on Tailscale and Twingate, it never
 *  starts, stops, or reconfigures them. */

import { invoke } from "@tauri-apps/api/core";
import type { PeerListing, VpnOverview } from "./types";

/**
 * Every VPN client's state, plus which of the given addresses each owns.
 *
 * Infallible on the Rust side — a machine with no VPNs answers with two
 * "not installed" entries and no bindings rather than an error.
 */
export const vpnOverview = (hostnames: string[]) =>
  invoke<VpnOverview>("vpn_overview", { hostnames });

/** The tailnet's other machines, for importing as hosts. Reports only. */
export const tailscalePeers = () => invoke<PeerListing>("tailscale_peers");
