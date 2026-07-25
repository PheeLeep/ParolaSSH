import { Shield, TriangleAlert } from "lucide-react";
import { conflictNote, VPN_LABELS } from "./types";
import { useVpn } from "./VpnProvider";

/**
 * One pill summarising every VPN on the machine; clicking opens the VPN page.
 *
 * Four states, quietest first: nothing rendered when no client is installed
 * (for most users this indicator does not exist), a muted "VPN" when clients
 * are installed but none is up, the connected VPN's name when exactly one is,
 * and an amber caution when several are up at once — the one state that can
 * actually break connections, so it is the one allowed to raise its voice.
 */
export function VpnIndicator({ onOpen }: { onOpen: () => void }) {
  const { statuses } = useVpn();
  const installed = statuses.filter((status) => status.installed);
  if (installed.length === 0) return null;

  const up = installed.filter((status) => status.up);

  let variant = "";
  let Icon = Shield;
  let text = "VPN";
  let title = installed
    .map((status) => `${VPN_LABELS[status.kind]} — ${status.detail}`)
    .join(" · ");

  if (up.length === 1) {
    variant = " status-badge--connected";
    text = VPN_LABELS[up[0].kind];
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
