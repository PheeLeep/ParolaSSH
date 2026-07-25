import {
  CircleAlert,
  CircleHelp,
  Info,
  Link2Off,
  Lock,
  LockOpen,
  OctagonAlert,
  Radiation,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestion,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
import {
  describePairing,
  describePermissions,
  explainPairing,
  isExposed,
  SEVERITY_LABELS,
  type KeyPermissions,
  type PublicKeyPairing,
  type Severity,
} from "./types";

/** An escalating ladder of shapes, so severity reads before colour does —
 *  which also keeps it legible for red/green colour blindness. */
export const SEVERITY_ICONS: Record<Severity, LucideIcon> = {
  critical: Radiation,
  high: OctagonAlert,
  medium: TriangleAlert,
  low: CircleAlert,
  info: Info,
};

export function SeverityIcon({
  severity,
  className,
}: {
  severity: Severity;
  className?: string;
}) {
  const Icon = SEVERITY_ICONS[severity];
  return <Icon className={className} aria-hidden="true" />;
}

/** Tinted pill, matching the tone of `StatusBadge` in features/hosts. */
export function SeverityBadge({ severity }: { severity: Severity }) {
  return (
    <span className={`severity-badge severity-badge--${severity}`}>
      <SeverityIcon severity={severity} className="icon-sm" />
      {SEVERITY_LABELS[severity]}
    </span>
  );
}

export function SeverityDot({ severity }: { severity: Severity }) {
  return (
    <span
      className={`severity-dot severity-dot--${severity}`}
      role="img"
      aria-label={SEVERITY_LABELS[severity]}
      title={SEVERITY_LABELS[severity]}
    />
  );
}

/** Whether the `.pub` sidecar is really this key's public half. */
export function PairingBadge({ pairing }: { pairing: PublicKeyPairing }) {
  const tone =
    pairing.kind === "matched"
      ? "safe"
      : pairing.kind === "mismatched"
        ? "exposed"
        : "unknown";

  const Icon =
    pairing.kind === "matched"
      ? ShieldCheck
      : pairing.kind === "mismatched"
        ? Link2Off
        : CircleHelp;

  return (
    <span className={`perm-badge perm-badge--${tone}`} title={explainPairing(pairing)}>
      <Icon className="icon-sm" aria-hidden="true" />
      {describePairing(pairing)}
    </span>
  );
}

/** Permissions rendered as a verdict rather than a raw mode.
 *  "Unknown" is shown as its own state — never as "fine". */
export function PermissionsBadge({ permissions }: { permissions: KeyPermissions }) {
  const exposed = isExposed(permissions);
  const description = describePermissions(permissions);

  if (exposed === null) {
    return (
      <span className="perm-badge perm-badge--unknown" title={description}>
        <ShieldQuestion className="icon-sm" aria-hidden="true" />
        Unknown
      </span>
    );
  }

  const Icon = exposed ? ShieldAlert : ShieldCheck;
  return (
    <span
      className={`perm-badge perm-badge--${exposed ? "exposed" : "safe"}`}
      title={description}
    >
      <Icon className="icon-sm" aria-hidden="true" />
      {permissions.kind === "posix" ? permissions.display : exposed ? "Exposed" : "Owner only"}
    </span>
  );
}

export function EncryptionBadge({ encrypted }: { encrypted: boolean }) {
  const Icon = encrypted ? Lock : LockOpen;
  return (
    <span
      className={`perm-badge perm-badge--${encrypted ? "safe" : "exposed"}`}
      title={encrypted ? "Protected by a passphrase" : "No passphrase"}
    >
      <Icon className="icon-sm" aria-hidden="true" />
      {encrypted ? "Encrypted" : "No passphrase"}
    </span>
  );
}

/** Fingerprints are long and identical up to the last few characters, so the
 *  tail is what the eye actually compares. */
export function Fingerprint({ value }: { value: string | null }) {
  if (!value) return <span className="text-body-secondary">—</span>;

  return (
    <code className="fingerprint small" title={value}>
      {value}
    </code>
  );
}
