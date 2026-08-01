/** `none` sends no credential: for servers that identify you before SSH
 *  begins, as Tailscale SSH does over WireGuard. */
export type AuthMethod = "password" | "publickey" | "agent" | "none";

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
  /** Id of the saved connection to tunnel through, or null for a direct
   *  connection. Ssh's `ProxyJump`, pointed at a host we already hold. */
  proxyJump: string | null;
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
  proxyJump: string | null;
}

/** What connecting must ask for before a private key can be loaded.
 *  `notNeeded` matters most: an unencrypted key must not raise a prompt. */
export type PassphraseNeed =
  | { kind: "notNeeded" }
  | { kind: "required" }
  | { kind: "hardware"; algorithm: string }
  | { kind: "unknown"; detail: string };

/** How an account gains permission to power the machine off. */
export type Elevation =
  | { kind: "notNeeded" }
  | { kind: "sudoNoPassword" }
  | { kind: "sudoPassword" }
  | { kind: "windowsAdminToken" }
  | { kind: "unavailable"; reason: string };

/** What the first key exchange negotiated - the audit tab's free tier. */
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

/* ── Files (SFTP) ──────────────────────────────────────────────────────── */

/** `symlink` and `other` exist to be refused, not opened - keeping them as
 *  distinct kinds is what lets a row explain why it is inert. */
export type EntryKind = "dir" | "file" | "symlink" | "other";

export interface RemoteEntry {
  name: string;
  path: string;
  kind: EntryKind;
  size: number;
  /** Unix seconds, or null when the server sent no mtime. */
  modified: number | null;
  /** Permission bits, or null on a server with no Unix modes. */
  mode: number | null;
  /** Where a symlink points. Shown, never followed. */
  target: string | null;
}

export interface DirListing {
  path: string;
  entries: RemoteEntry[];
  /** The directory held more entries than one listing returns. */
  truncated: boolean;
}

/** One file found by a recursive walk. `relative` mirrors the tree under the
 *  folder the walk started at. */
export interface TreeFile {
  path: string;
  relative: string;
  size: number;
}

export interface TreeListing {
  files: TreeFile[];
  /** Symlinks and device files passed over, so the UI can say what was left
   *  out rather than transferring less than the user saw. */
  skipped: string[];
  truncated: boolean;
}

/* ── Transfers ─────────────────────────────────────────────────────────── */

/** What to do when a destination is already taken. Decided by the user before
 *  anything is queued - the backend has nobody to ask. */
export type OnConflict = "keepBoth" | "overwrite";


export type TransferDirection = "upload" | "download";
export type TransferPriority = "low" | "normal" | "high";
export type TransferState =
  | "queued"
  | "running"
  | "done"
  | "failed"
  | "canceled";

export interface TransferRecord {
  id: number;
  hostId: string;
  /** Denormalized, so a row still names its host after disconnecting. */
  hostLabel: string;
  direction: TransferDirection;
  remotePath: string;
  localPath: string;
  name: string;
  priority: TransferPriority;
  state: TransferState;
  bytesDone: number;
  bytesTotal: number | null;
  /** 1-based place among everything waiting; null once running or settled. */
  queuePosition: number | null;
  error: string | null;
  queuedAt: string;
  startedAt: string | null;
  finishedAt: string | null;
}

export interface TransferProgress {
  transferId: number;
  hostId: string;
  bytesDone: number;
  bytesTotal: number | null;
  state: TransferState;
}

export interface TransferSummary {
  running: number;
  queued: number;
  maxConcurrent: number;
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
  checkedAtMs: number;
}

/* ── Tasks ─────────────────────────────────────────────────────────────── */

/** Where a task appears. `global` still runs on exactly one host - the one
 *  being looked at - it is only offered everywhere. */
export type TaskScope = { kind: "global" } | { kind: "host"; hostId: string };

/** How much a command deserves to be stopped at. `none` is "nothing matched",
 *  never "this is safe" - see the module doc on the Rust side. */
export type DangerLevel = "none" | "caution" | "destructive";

export interface DangerReason {
  label: string;
  detail: string;
  level: DangerLevel;
}

export interface DangerAssessment {
  level: DangerLevel;
  reasons: DangerReason[];
}

/** A task the app ships, already resolved to this host's OS. */
export interface BuiltinTask {
  id: string;
  name: string;
  description: string;
  command: string;
  elevated: boolean;
}

/** A task the operator saved. `command` is their text, run verbatim. */
export interface TaskRecord {
  id: string;
  name: string;
  description: string | null;
  command: string;
  elevated: boolean;
  scope: TaskScope;
  /** Empty means every family. */
  osFamilies: OsFamily[];
  createdAt: string | null;
  lastRunAt: string | null;
}

export interface TaskDraft {
  id?: string | null;
  name: string;
  description?: string | null;
  command: string;
  elevated: boolean;
  scope: TaskScope;
  osFamilies: OsFamily[];
}

export interface HostTasks {
  os: OsFamily;
  builtin: BuiltinTask[];
  saved: TaskRecord[];
}

/** Exactly what a press would run. `command` includes the sudo wrapper; the
 *  pane shows both so a long `sudo sh -c '…'` stays readable. */
export interface TaskPlan {
  command: string;
  innerCommand: string;
  elevated: boolean;
  needsPassword: boolean;
  danger: DangerAssessment;
}

export const AUTH_METHOD_LABELS: Record<AuthMethod, string> = {
  password: "Password",
  publickey: "Public key",
  agent: "SSH agent",
  none: "None (Tailscale SSH)",
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
  notNeeded: "Root - no elevation needed",
  sudoNoPassword: "sudo (no password)",
  sudoPassword: "sudo (password required)",
  windowsAdminToken: "Administrator token",
  unavailable: "Cannot elevate",
};

/* ── Tunnels (port forwarding) ────────────────────────────────────────── */

export interface TunnelInfo {
  id: number;
  hostId: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  activeConnections: number;
}

export interface TunnelEvent {
  hostId: string;
  tunnelId: number;
  kind: string;
}

/* ── Importing from ~/.ssh/config ──────────────────────────────────────── */

/** One `Host` block that names a single machine. */
export interface ImportCandidate {
  alias: string;
  hostname: string;
  port: number;
  /** Empty when the config names no `User`. */
  username: string;
  keyPath: string | null;
  /** Alias named by `ProxyJump`, matched to another candidate on import. */
  proxyJump: string | null;
  line: number;
  notes: string[];
}

export interface ImportListing {
  path: string;
  /** False means no config file at all, which is not an empty one. */
  exists: boolean;
  candidates: ImportCandidate[];
  notes: string[];
}

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
    proxyJump: null,
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
    proxyJump: host.proxyJump,
  };
}
