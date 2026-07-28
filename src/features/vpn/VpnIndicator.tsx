import { Shield, TriangleAlert } from "lucide-react";
import { conflictNote, VPN_LABELS } from "./types";
import { useVpn } from "./VpnProvider";

/** One pill summarising every VPN on the machine; clicking opens the VPN page.
 *  Four states, quietest first: nothing when no client is installed, a muted
 *  "VPN" when none is up, the VPN's name when exactly one is, and an amber
 *  caution when several are - the only state that can break connections. */
export function VpnIndicator({ onOpen }: { onOpen: () => void }) {
  const { statuses } = useVpn();
  const installed = statuses.filter((status) => status.installed);
  if (installed.length === 0) return null;

  const up = installed.filter((status) => status.up);
  const idle = installed.length - up.length;

  let variant = "";
  let Icon = Shield;
  let text = "VPN";
  // Every client's own wording is what the VPN page is for; the pill only has
  // to say which one is carrying traffic and how many are sitting out.
  let title = `${installed.map((status) => VPN_LABELS[status.kind]).join(", ")} - none connected`;

  if (up.length === 1) {
    variant = " status-badge--connected";
    text = VPN_LABELS[up[0].kind];
    title = `${VPN_LABELS[up[0].kind]} - ${up[0].detail}`;
    if (idle > 0) title += ` · ${idle} idle`;
  } else if (up.length >= 2) {
    variant = " status-badge--warning";
    Icon = TriangleAlert;
    text = `${up.length} VPNs active`;
    title = conflictNote(up.map((status) => VPN_LABELS[status.kind]));
  }

  return (
    <button
      type="button"
      className={`vpn-pill status-badge${variant}`}
      onClick={onOpen}
      title={title}
      aria-label={`VPN status: ${title}. Open the VPN page.`}
    >
      <Icon className="icon-sm" aria-hidden="true" />
      {text}
    </button>
  );
}
