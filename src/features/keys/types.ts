/** Mirrors the serde shapes in `src-tauri/src/ssh/`. Kept hand-written to
 *  match the convention in `features/hosts/types.ts`; if these grow much
 *  further, generate them instead. */

export type KeyFormat = "openSsh" | "legacyPem" | "pkcs8" | "unknown";

export type KdfInfo =
  | { kind: "none" }
  | { kind: "bcrypt"; rounds: number }
  | { kind: "legacyPemMd5"; cipher: string }
  | { kind: "unknown" };

/** Permissions are modelled per-platform: a mode integer means nothing on
 *  Windows, so the audit branches on `kind` rather than faking one. */
export type KeyPermissions =
  | { kind: "posix"; mode: number; display: string }
  | { kind: "windows"; principals: string[]; inherited: boolean }
  | { kind: "unknown"; reason: string };

/** Whether the `.pub` sidecar actually belongs to the private key.
 *  A stale sidecar is what you deployed to servers, so a mismatch means auth
 *  fails with no useful error. */
export type PublicKeyPairing =
  | { kind: "matched" }
  | {
      kind: "mismatched";
      privateFingerprint: string;
      publicFingerprint: string;
    }
  | { kind: "missing" }
  | { kind: "unreadable"; reason: string }
  | { kind: "unverifiable" };

export interface SshKey {
  id: string;
  path: string;
  fileName: string;
  algorithmId: string;
  algorithm: string;
  bits: number | null;
  fingerprint: string | null;
  comment: string | null;
  encrypted: boolean;
  kdf: KdfInfo;
  format: KeyFormat;
  publicKeyPath: string | null;
  publicKeyOpenssh: string | null;
  pairing: PublicKeyPairing;
  permissions: KeyPermissions;
  modifiedMs: number | null;
  parseError: string | null;
}

export interface OrphanPublicKey {
  path: string;
  fileName: string;
  fingerprint: string | null;
  comment: string | null;
}

export interface KeyScan {
  keys: SshKey[];
  orphanPublicKeys: OrphanPublicKey[];
}

export type Severity = "critical" | "high" | "medium" | "low" | "info";

export const SEVERITY_ORDER: Severity[] = [
  "critical",
  "high",
  "medium",
  "low",
  "info",
];

export const SEVERITY_LABELS: Record<Severity, string> = {
  critical: "Critical",
  high: "High",
  medium: "Medium",
  low: "Low",
  info: "Info",
};

export type Remediation =
  | { kind: "restrictPermissions"; path: string; isDir: boolean }
  | { kind: "manual"; instruction: string };

export interface Finding {
  id: string;
  ruleId: string;
  severity: Severity;
  title: string;
  detail: string;
  location: string;
  path: string | null;
  remediation: Remediation | null;
  suppressed: boolean;
}

export interface SeverityCounts {
  critical: number;
  high: number;
  medium: number;
  low: number;
  info: number;
}

export interface AuditReport {
  sshDir: string;
  dirExists: boolean;
  symlinkTarget: string | null;
  directoryPermissions: KeyPermissions;
  findings: Finding[];
  counts: SeverityCounts;
  score: number;
  keyCount: number;
  scannedAtMs: number;
}

export interface SshLocation {
  path: string;
  exists: boolean;
}

export interface GenerateRequest {
  algorithm: string;
  bits?: number | null;
  fileName: string;
  comment?: string | null;
  passphrase?: string | null;
  overwrite?: boolean;
}

export interface GenerateOutcome {
  key: SshKey;
  privateKeyPath: string;
  publicKeyPath: string;
  publicKeyOpenssh: string;
}

/** Options offered by the create-key dialog. */
export const KEY_ALGORITHMS = [
  {
    id: "ed25519",
    label: "Ed25519",
    hint: "Recommended. Small, fast, and the modern default.",
    sizes: null,
  },
  {
    id: "rsa",
    label: "RSA",
    hint: "Widest compatibility with older servers.",
    sizes: [3072, 4096],
  },
  {
    id: "ecdsa",
    label: "ECDSA",
    hint: "NIST curves. Prefer Ed25519 unless something requires this.",
    sizes: [256, 384, 521],
  },
] as const;

export function describePermissions(permissions: KeyPermissions): string {
  switch (permissions.kind) {
    case "posix":
      return permissions.display;
    case "windows":
      return permissions.principals.length === 0
        ? "No access entries"
        : permissions.principals.join(", ");
    case "unknown":
      return permissions.reason;
  }
}

/** Whether anyone but the owner can reach the file.
 *  `null` means undetermined — deliberately distinct from "safe". */
export function isExposed(permissions: KeyPermissions): boolean | null {
  switch (permissions.kind) {
    case "posix":
      return (permissions.mode & 0o077) !== 0;
    case "windows":
      return permissions.inherited || permissions.principals.length > 3;
    case "unknown":
      return null;
  }
}

export function describeKdf(kdf: KdfInfo): string {
  switch (kdf.kind) {
    case "none":
      return "No passphrase";
    case "bcrypt":
      return `bcrypt, ${kdf.rounds} rounds`;
    case "legacyPemMd5":
      return `Legacy PEM (${kdf.cipher}, MD5)`;
    case "unknown":
      return "Unknown";
  }
}

export const KEY_FORMAT_LABELS: Record<KeyFormat, string> = {
  openSsh: "OpenSSH",
  legacyPem: "Legacy PEM",
  pkcs8: "PKCS#8",
  unknown: "Unknown",
};

/** Epoch millis from Rust → the ISO strings `lib/format.ts` expects. */
export function toIso(ms: number | null): string | null {
  return ms === null ? null : new Date(ms).toISOString();
}

/** Short verdict for the keys table. */
export function describePairing(pairing: PublicKeyPairing): string {
  switch (pairing.kind) {
    case "matched":
      return "Matched";
    case "mismatched":
      return "Mismatched";
    case "missing":
      return "No .pub";
    case "unreadable":
      return "Unreadable";
    case "unverifiable":
      return "Unverified";
  }
}

/** The sentence explaining what the verdict means. */
export function explainPairing(pairing: PublicKeyPairing): string {
  switch (pairing.kind) {
    case "matched":
      return "The .pub file is the public half of this private key.";
    case "mismatched":
      return `The .pub file is a different key (${pairing.publicFingerprint}). Anywhere you installed it will reject this key.`;
    case "missing":
      return "There is no .pub file. It can be regenerated from the private key at any time.";
    case "unreadable":
      return pairing.reason;
    case "unverifiable":
      return "The private key could not be parsed, so the .pub file cannot be checked against it.";
  }
}
