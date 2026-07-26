export type VpnKind =
  | "tailscale"
  | "twingate"
  | "netbird"
  | "zerotier"
  | "wireguard";

/** One VPN client's condition on the local machine. */
export interface VpnStatus {
  kind: VpnKind;
  /** Whether the client software exists on this machine at all. */
  installed: boolean;
  /** Whether it currently provides connectivity. Best-effort on platforms
   *  where the client offers no status command. */
  up: boolean;
  /** Plain-language state, ready to show: "connected", "needs login", … */
  detail: string;
}

/** One saved address's relationship to a VPN. */
export interface VpnBinding {
  hostname: string;
  /** null when the address sits in a range either VPN could own. */
  kind: VpnKind | null;
  /** Ready to show, e.g. "Twingate resource 'staging'". */
  description: string;
}

/** One Twingate resource, as the client reports it. */
export interface VpnResource {
  name: string;
  /** A single IP, a CIDR range, or a `*.domain` wildcard. */
  address: string;
  alias: string | null;
  /** The client's own words, e.g. "Auth expires in 4 days". */
  authStatus: string;
  /** Whether access would be refused right now. */
  needsAuth: boolean;
}

export interface VpnOverview {
  statuses: VpnStatus[];
  bindings: VpnBinding[];
  /** Empty off Linux or while the Twingate service is stopped. */
  twingateResources: VpnResource[];
}

/** One machine on the tailnet, as `tailscale status --json` reports it. */
export interface TailscalePeer {
  hostName: string;
  /** What a saved host connects to: MagicDNS name, else the 100.x address. */
  address: string;
  dnsName: string | null;
  tailscaleIp: string | null;
  /** Tailscale's own word: `linux`, `windows`, `macOS`, `iOS`… */
  os: string;
  online: boolean;
  /** ACL tags with the `tag:` prefix already stripped. */
  tags: string[];
}

/** Why there is no peer list, or the list. An empty tailnet and a logged-out
 *  client are different answers and must not render the same. */
export type PeerListing =
  | { kind: "notInstalled" }
  | { kind: "unavailable"; detail: string }
  | { kind: "peers"; peers: TailscalePeer[] };

export const VPN_LABELS: Record<VpnKind, string> = {
  tailscale: "Tailscale",
  twingate: "Twingate",
  netbird: "NetBird",
  zerotier: "ZeroTier",
  wireguard: "WireGuard",
};

/** The caution shown when several VPNs are connected at once. A caution, not
 *  an error: two VPNs can coexist on different routes, but Twingate itself
 *  warns when started beside another. */
export function conflictNote(names: string[]): string {
  return (
    `${names.join(" and ")} are connected at the same time — they can compete ` +
    "for routes and DNS. Disconnect one if connections misbehave."
  );
}
