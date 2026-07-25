import { ShieldCheck } from "lucide-react";
import { useVpn } from "./VpnProvider";

/**
 * A small shield after an address that is reached through a VPN.
 *
 * Renders nothing for ordinary addresses, so rows only differ where the
 * difference is real. The tooltip carries the specifics ("Twingate resource
 * 'staging'") because the glyph itself is deliberately too small to.
 */
export function VpnGlyph({ hostname }: { hostname: string }) {
  const { bindingFor } = useVpn();
  const binding = bindingFor(hostname);
  if (!binding) return null;

  return (
    <span
      className="vpn-glyph"
      role="img"
      aria-label={binding.description}
      title={binding.description}
    >
      <ShieldCheck className="icon-sm" aria-hidden="true" />
    </span>
  );
}
