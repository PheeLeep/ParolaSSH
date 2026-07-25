export type AuthMethod = "password" | "publickey" | "agent";

export type HostStatus = "connected" | "reachable" | "offline" | "unknown";

export type OsFamily = "linux" | "macos" | "bsd" | "windows" | "unknown";

/** A saved connection, as stored by the Rust side. Holds no secrets. */
export interface SshHost {
  id: string;
  /** Friendly name shown in the list. */
  label: string;
  hostname: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  /** Private key offered when `authMethod` is `publickey`. */
  keyPath: string | null;
  /** Folder / environment this host belongs to. */
  group: string;
  tags: string[];
  notes: string | null;
  /** ISO 8601, or null if never connected. */
  lastConnected: string | null;
}

/** What the add/edit form sends. `id` present means edit, absent means add. */
export interface HostDraft {
  id?: string;
  label: string;
  hostname: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  group: string;
  tags: string[];
  notes: string | null;
}

/** How an account gains permission to power the machine off. */
export type Elevation =
  | { kind: "notNeeded" }
  | { kind: "sudoNoPassword" }
  | { kind: "sudoPassword" }
  | { kind: "windowsAdminToken" }
  | { kind: "unavailable"; reason: string };

/** What the first key exchange negotiated — the audit tab's free tier. */
export interface NegotiatedCrypto {
  kex: string;
  hostKeyAlgorithm: string;
  cipher: string;
  clientMac: string;
  serverMac: string;
  strictKex: boolean;
}

/** A live SSH session's details. */
export interface ConnectionInfo {
  hostId: string;
  connected: boolean;
  os: OsFamily;
  osDetail: string;
  user: string;
  elevation: Elevation;
  elevationExplanation: string;
  supportsForce: boolean;
  supportsCancel: boolean;
  fingerprint: string | null;
  negotiated: NegotiatedCrypto | null;
  connectedAt: string;
  shellIds: number[];
  hasLoginPassword: boolean;
}

export interface HostHealth {
  hostId: string;
  connected: boolean;
  reachable: boolean;
  latencyMs: number | null;
}

export interface ProbeResult {
  hostname: string;
  port: number;
  reachable: boolean;
  isSsh: boolean;
  banner: string | null;
  latencyMs: number | null;
  message: string;
}

export type PowerAction = "shutdown" | "reboot" | "cancel";

export interface PowerRequest {
  action: PowerAction;
  delayMinutes: number;
  force: boolean;
  message: string | null;
}

export interface PowerPlan {
  command: string;
  needsPassword: boolean;
  summary: string;
}

export interface PowerOutcome {
  command: string;
  summary: string;
  succeeded: boolean;
  message: string;
  stdout: string;
  stderr: string;
  exitCode: number | null;
}

export interface CommandOutput {
  stdout: string;
  stderr: string;
  exitCode: number | null;
}

export interface TerminalOutput {
  hostId: string;
  shellId: number;
  stderr: boolean;
  chunk: string;
}

export interface TerminalClosed {
  hostId: string;
  shellId: number;
  exitCode: number | null;
}

/* ── Streams (followed logs) ───────────────────────────────────────────── */

export interface StreamOutput {
  hostId: string;
  streamId: number;
  stderr: boolean;
  chunk: string;
}

export interface StreamClosed {
  hostId: string;
  streamId: number;
  exitCode: number | null;
}

/* ── Services ──────────────────────────────────────────────────────────── */

export type ServiceState = "running" | "stopped" | "failed" | "other";

export interface ServiceEntry {
  name: string;
  description: string;
  state: ServiceState;
  detail: string;
}

export type ServiceAction = "start" | "stop" | "restart";

export interface ServiceActionRequest {
  action: ServiceAction;
  unit: string;
}

export interface ServicePlan {
  command: string;
  needsPassword: boolean;
  summary: string;
}

export interface ServiceOutcome {
  command: string;
  summary: string;
  succeeded: boolean;
  message: string;
  stdout: string;
  stderr: string;
  exitCode: number | null;
}

export interface ServiceLog {
  lines: string[];
  note: string | null;
}

export const SERVICE_STATE_LABELS: Record<ServiceState, string> = {
  running: "Running",
  stopped: "Stopped",
  failed: "Failed",
  other: "Other",
};

/* ── Performance ───────────────────────────────────────────────────────── */

export interface MemoryInfo {
  totalKb: number;
  availableKb: number;
  usedPercent: number;
}

export interface DiskInfo {
  mount: string;
  totalKb: number;
  usedKb: number;
  usedPercent: number;
}

export interface HostMetrics {
  sampledAtMs: number;
  cpuPercent: number | null;
  memory: MemoryInfo | null;
  load: [number, number, number] | null;
  uptimeSeconds: number | null;
  disks: DiskInfo[];
  notes: string[];
}

/* ── Updates ───────────────────────────────────────────────────────────── */

export interface UpdateItem {
  name: string;
  current: string | null;
  available: string;
  source: string;
  security: boolean;
}

export interface HotfixItem {
  id: string;
  description: string;
  installedOn: string | null;
}

export type UpdateReport =
  | { kind: "list"; manager: string; updates: UpdateItem[]; securityCount: number | null }
  | { kind: "upToDate"; manager: string }
  | { kind: "managerMissing"; detail: string }
  | { kind: "moduleMissing"; detail: string; installedHistory: HotfixItem[] };

/* ── Remote audit ──────────────────────────────────────────────────────── */

export type RemoteSeverity = "info" | "low" | "medium" | "high" | "critical";

export interface RemoteFinding {
  id: string;
  ruleId: string;
  severity: RemoteSeverity;
  title: string;
  detail: string;
  location: string;
  /** Shown, never executed by the app. */
  instruction: string | null;
  suppressed: boolean;
}

export interface RemoteSeverityCounts {
  critical: number;
  high: number;
  medium: number;
  low: number;
  info: number;
}

export interface RemoteAuditReport {
  hostId: string;
  findings: RemoteFinding[];
  counts: RemoteSeverityCounts;
  score: number;
  tier1Ran: boolean;
  tier1Note: string | null;
  lynis: string | null;
  checkedAtMs: number;
}

export const AUTH_METHOD_LABELS: Record<AuthMethod, string> = {
  password: "Password",
  publickey: "Public key",
  agent: "SSH agent",
};

export const STATUS_LABELS: Record<HostStatus, string> = {
  connected: "Connected",
  reachable: "Reachable",
  offline: "Offline",
  unknown: "Unknown",
};

export const OS_LABELS: Record<OsFamily, string> = {
  linux: "Linux",
  macos: "macOS",
  bsd: "BSD",
  windows: "Windows",
  unknown: "Unknown",
};

export const ELEVATION_LABELS: Record<Elevation["kind"], string> = {
  notNeeded: "Root — no elevation needed",
  sudoNoPassword: "sudo (no password)",
  sudoPassword: "sudo (password required)",
  windowsAdminToken: "Administrator token",
  unavailable: "Cannot elevate",
};

export const DEFAULT_PORT = 22;
export const DEFAULT_GROUP = "Ungrouped";

export function emptyDraft(group = DEFAULT_GROUP): HostDraft {
  return {
    label: "",
    hostname: "",
    port: DEFAULT_PORT,
    username: "",
    authMethod: "password",
    keyPath: null,
    group,
    tags: [],
    notes: null,
  };
}

export function draftFromHost(host: SshHost): HostDraft {
  return {
    id: host.id,
    label: host.label,
    hostname: host.hostname,
    port: host.port,
    username: host.username,
    authMethod: host.authMethod,
    keyPath: host.keyPath,
    group: host.group,
    tags: [...host.tags],
    notes: host.notes,
  };
}
