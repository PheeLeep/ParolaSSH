/** The Rust command surface for saved connections and live sessions.
 *
 *  Passwords are passed as arguments and never returned - no command here
 *  reads one back, and nothing writes a secret to disk. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ConnectionInfo,
  DangerAssessment,
  DirListing,
  HostTasks,
  TaskDraft,
  TaskPlan,
  TaskRecord,
  HostDraft,
  ImportListing,
  OnConflict,
  HostHealth,
  HostMetrics,
  PassphraseNeed,
  PowerOutcome,
  PowerPlan,
  PowerRequest,
  ProbeResult,
  RemoteAuditReport,
  ServiceActionRequest,
  ServiceEntry,
  ServiceLog,
  ServiceOutcome,
  ServicePlan,
  SshHost,
  StreamClosed,
  StreamOutput,
  TerminalClosed,
  TerminalOutput,
  TransferPriority,
  TransferProgress,
  TransferRecord,
  TransferSummary,
  TunnelEvent,
  TunnelInfo,
  TreeListing,
  UpdateReport,
} from "./types";

/* ── Saved connections ─────────────────────────────────────────────────── */

export const listHosts = () => invoke<SshHost[]>("list_hosts");

/** Adds when the draft has no id, updates when it does. */
export const saveHost = (draft: HostDraft) =>
  invoke<SshHost>("save_host", { draft });

/** Returns the record that was removed. */
export const deleteHost = (id: string) => invoke<SshHost>("delete_host", { id });

export const listHostGroups = () => invoke<string[]>("list_host_groups");

export const listHostTags = () => invoke<string[]>("list_host_tags");

/** Hosts defined in `~/.ssh/config`, as importable connections. */
export const sshConfigHosts = () => invoke<ImportListing>("ssh_config_hosts");

/* ── Reaching them ─────────────────────────────────────────────────────── */

/** Is anything listening on that port, and does it speak SSH?
 *  When `username` is provided, also detects supported auth methods. */
export const probeHost = (hostname: string, port: number, username?: string) =>
  invoke<ProbeResult>("probe_host", { hostname, port, username: username || null });

/** Connect and authenticate. `trustUnknown` records an unrecognised host key -
 *  pass it only once the user has seen and accepted the fingerprint. */
export const connectHost = (
  hostId: string,
  options: {
    password?: string | null;
    remember?: boolean;
    trustUnknown?: boolean;
  } = {},
) =>
  invoke<ConnectionInfo>("connect_host", {
    hostId,
    password: options.password ?? null,
    remember: options.remember ?? false,
    trustUnknown: options.trustUnknown ?? false,
  });

/** Tell the tray a connect dialog is open, so its icon blinks while the user
 *  is being asked for a credential. Rust cannot see a dialog by itself. */
export const setConnectPending = (hostId: string, active: boolean) =>
  invoke<void>("set_connect_pending", { hostId, active });

/** Forget every open dialog - a reload leaves none of them on screen. */
export const clearConnectPending = () => invoke<void>("clear_connect_pending");

export const disconnectHost = (hostId: string) =>
  invoke<boolean>("disconnect_host", { hostId });

export const connectedHosts = () => invoke<string[]>("connected_hosts");

export const hasRememberedPassword = (hostId: string) =>
  invoke<boolean>("has_remembered_password", { hostId });

export const forgetPassword = (hostId: string) =>
  invoke<void>("forget_password", { hostId });

/** Whether this host's key is locked, so the dialog can skip a prompt with
 *  nothing to unlock. Returns no key material. */
export const hostKeyPassphraseNeed = (hostId: string) =>
  invoke<PassphraseNeed>("host_key_passphrase_need", { hostId });

/** Check every saved host: are they up, and are our sessions still good?
 *  Failed sessions are dropped by the Rust side, so the result is
 *  authoritative. */
export const heartbeat = () => invoke<HostHealth[]>("heartbeat");

/* ── Power ─────────────────────────────────────────────────────────────── */

/** The exact command a request would run, without running it. */
export const previewPower = (hostId: string, request: PowerRequest) =>
  invoke<PowerPlan>("preview_power", { hostId, request });

export const powerHost = (
  hostId: string,
  request: PowerRequest,
  password?: string | null,
) =>
  invoke<PowerOutcome>("power_host", {
    hostId,
    request,
    password: password ?? null,
  });

/* ── Interactive terminal ──────────────────────────────────────────────── */

/** Open a shell and return its id. A host can hold several terminals on the one
 *  connection; the id addresses every later call and tags every event. */
export const openShell = (hostId: string, cols: number, rows: number) =>
  invoke<number>("open_shell", { hostId, cols, rows });

/** Shell ids already open on a host, so a reload can rebuild its tabs. */
export const listShells = (hostId: string) =>
  invoke<number[]>("list_shells", { hostId });

export const writeShell = (hostId: string, shellId: number, data: string) =>
  invoke<void>("write_shell", { hostId, shellId, data });

export const broadcastShells = (
  targets: { hostId: string; shellId: number }[],
  data: string,
) => invoke<void>("broadcast_shells", { targets, data });

export const resizeShell = (
  hostId: string,
  shellId: number,
  cols: number,
  rows: number,
) => invoke<void>("resize_shell", { hostId, shellId, cols, rows });

/** Close one shell. An id that has already gone is a no-op, not an error. */
export const closeShell = (hostId: string, shellId: number) =>
  invoke<void>("close_shell", { hostId, shellId });

/** Subscribe to terminal output for one host. `isMine` picks which shell's
 *  bytes to render: host id alone is not enough, since a replaced shell keeps
 *  streaming and its output would read as duplicates in the new pane. */
export function onTerminalOutput(
  hostId: string,
  isMine: (shellId: number) => boolean,
  handler: (event: TerminalOutput) => void,
): Promise<UnlistenFn> {
  return listen<TerminalOutput>("terminal://output", ({ payload }) => {
    if (payload.hostId === hostId && isMine(payload.shellId)) handler(payload);
  });
}

export function onTerminalClosed(
  hostId: string,
  isMine: (shellId: number) => boolean,
  handler: (event: TerminalClosed) => void,
): Promise<UnlistenFn> {
  return listen<TerminalClosed>("terminal://closed", ({ payload }) => {
    if (payload.hostId === hostId && isMine(payload.shellId)) handler(payload);
  });
}

/* ── Services ──────────────────────────────────────────────────────────── */

export const listServices = (hostId: string) =>
  invoke<ServiceEntry[]>("list_services", { hostId });

/** The exact command an action would run, without running it. */
export const previewServiceAction = (
  hostId: string,
  request: ServiceActionRequest,
) => invoke<ServicePlan>("preview_service_action", { hostId, request });

export const serviceAction = (
  hostId: string,
  request: ServiceActionRequest,
  password?: string | null,
) =>
  invoke<ServiceOutcome>("service_action", {
    hostId,
    request,
    password: password ?? null,
  });

/** The last journal lines (Linux) or SCM events (Windows) for a service.
 *  `displayName` matters only on Windows, where events name services by it. */
export const serviceLog = (
  hostId: string,
  unit: string,
  displayName?: string | null,
) => invoke<ServiceLog>("service_log", { hostId, unit, displayName: displayName ?? null });

/** Follow a journal. Output arrives as `stream://output` events. */
export const followServiceLog = (hostId: string, unit: string) =>
  invoke<number>("follow_service_log", { hostId, unit });

/** Close one stream. An id already gone is a no-op, not an error. */
export const closeStream = (hostId: string, streamId: number) =>
  invoke<void>("close_stream", { hostId, streamId });

/** Same double filter as the terminal events, for the same reason. */
export function onStreamOutput(
  hostId: string,
  isMine: (streamId: number) => boolean,
  handler: (event: StreamOutput) => void,
): Promise<UnlistenFn> {
  return listen<StreamOutput>("stream://output", ({ payload }) => {
    if (payload.hostId === hostId && isMine(payload.streamId)) handler(payload);
  });
}

export function onStreamClosed(
  hostId: string,
  isMine: (streamId: number) => boolean,
  handler: (event: StreamClosed) => void,
): Promise<UnlistenFn> {
  return listen<StreamClosed>("stream://closed", ({ payload }) => {
    if (payload.hostId === hostId && isMine(payload.streamId)) handler(payload);
  });
}

/* ── Performance ───────────────────────────────────────────────────────── */

/** One sample. The pane polls this only while it is visible. */
export const sampleMetrics = (hostId: string) =>
  invoke<HostMetrics>("sample_metrics", { hostId });

/* ── Updates ───────────────────────────────────────────────────────────── */

/** Read-only, always - there is no install command to call. */
export const checkUpdates = (hostId: string) =>
  invoke<UpdateReport>("check_updates", { hostId });

/* ── Remote audit ──────────────────────────────────────────────────────── */

/** Tiers 0–1. `password` feeds the sudo retry only, and
 *  `elevate: false` skips that retry even when the session holds a password. */
export const remoteAudit = (
  hostId: string,
  password?: string | null,
  elevate = true,
) =>
  invoke<RemoteAuditReport>("remote_audit", {
    hostId,
    password: password ?? null,
    elevate,
  });

export const setRemoteFindingSuppressed = (
  hostId: string,
  findingId: string,
  suppressed: boolean,
) => invoke<void>("set_remote_finding_suppressed", { hostId, findingId, suppressed });

/* ── Tasks ─────────────────────────────────────────────────────────────── */

/** What this host can run: the built-ins its OS supports, plus the saved
 *  tasks scoped to it. Safe to call while disconnected - the OS reads back as
 *  `unknown` and no built-in is offered rather than a guess. */
export const listHostTasks = (hostId: string) =>
  invoke<HostTasks>("list_host_tasks", { hostId });

/** Every saved task, for the list that manages them across hosts. */
export const listAllTasks = () => invoke<TaskRecord[]>("list_all_tasks");

export const saveTask = (draft: TaskDraft) =>
  invoke<TaskRecord>("save_task", { draft });

export const deleteTask = (id: string) => invoke<TaskRecord>("delete_task", { id });

/** Drop every task pinned to a host. Called when the host itself is deleted. */
export const forgetHostTasks = (hostId: string) =>
  invoke<number>("forget_host_tasks", { hostId });

/** Exactly what a press would run, and what the app makes of it. Runs nothing.
 *
 *  `elevated` overrides the task's own setting for this one press; leave it
 *  undefined to take the task's default. */
export const planTask = (hostId: string, taskId: string, elevated?: boolean) =>
  invoke<TaskPlan>("plan_task", { hostId, taskId, elevated: elevated ?? null });

/** Assess a command that has not been saved yet - what the editor calls as
 *  the operator types. */
export const assessTaskCommand = (command: string, hostId?: string | null) =>
  invoke<DangerAssessment>("assess_task_command", {
    hostId: hostId ?? null,
    command,
  });

/** Run a task. Resolves to the stream id; output arrives as `stream://output`
 *  events and `closeStream` stops watching.
 *
 *  The command is *not* sent - the backend rebuilds the plan from the task id,
 *  so a window showing one command can never submit another. */
export const startTask = (
  hostId: string,
  taskId: string,
  elevated?: boolean,
  password?: string | null,
) =>
  invoke<number>("start_task", {
    hostId,
    taskId,
    elevated: elevated ?? null,
    password: password ?? null,
  });

/* ── Files (SFTP) ──────────────────────────────────────────────────────── */

export const listRemoteDir = (hostId: string, path: string) =>
  invoke<DirListing>("list_remote_dir", { hostId, path });

/** Where a fresh browser opens - the subsystem's own answer, not a guess. */
export const remoteHomeDir = (hostId: string) =>
  invoke<string>("remote_home_dir", { hostId });

export const createRemoteDir = (hostId: string, path: string, name: string) =>
  invoke<string>("create_remote_dir", { hostId, path, name });

export const deleteRemoteEntry = (hostId: string, path: string, isDir: boolean) =>
  invoke<void>("delete_remote_entry", { hostId, path, isDir });

/** Rename or move - the same SFTP request, differing only in whether the
 *  destination's parent is the one it is already in. Never overwrites. */
export const renameRemoteEntry = (hostId: string, from: string, to: string) =>
  invoke<string>("rename_remote_entry", { hostId, from, to });

/** Copy within one host, done by the server so the bytes never cross the wire. */
export const copyRemoteEntry = (hostId: string, from: string, to: string) =>
  invoke<string>("copy_remote_entry", { hostId, from, to });

/** Every regular file under a folder, for a recursive transfer. */
export const listRemoteTree = (hostId: string, path: string) =>
  invoke<TreeListing>("list_remote_tree", { hostId, path });

/** Which of `names` already exist, so the user is asked before anything is
 *  queued rather than after something is lost. */
export const localConflicts = (localDir: string, names: string[]) =>
  invoke<string[]>("local_conflicts", { localDir, names });

export const remoteConflicts = (hostId: string, remoteDir: string, names: string[]) =>
  invoke<string[]>("remote_conflicts", { hostId, remoteDir, names });

/* ── Transfers ─────────────────────────────────────────────────────────── */

export const enqueueDownload = (
  hostId: string,
  remotePath: string,
  localDir: string,
  options: {
    /** Sub-path under `localDir`, so a folder transfer mirrors the tree. */
    relative?: string | null;
    onConflict?: OnConflict | null;
    priority?: TransferPriority | null;
  } = {},
) =>
  invoke<number>("enqueue_download", {
    hostId,
    remotePath,
    localDir,
    relative: options.relative ?? null,
    onConflict: options.onConflict ?? null,
    priority: options.priority ?? null,
  });

export const enqueueUpload = (
  hostId: string,
  localPath: string,
  remoteDir: string,
  options: { onConflict?: OnConflict | null; priority?: TransferPriority | null } = {},
) =>
  invoke<number>("enqueue_upload", {
    hostId,
    localPath,
    remoteDir,
    onConflict: options.onConflict ?? null,
    priority: options.priority ?? null,
  });

export const listTransfers = () => invoke<TransferRecord[]>("list_transfers");

export const transferSummary = () => invoke<TransferSummary>("transfer_summary");

export const cancelTransfer = (transferId: number) =>
  invoke<void>("cancel_transfer", { transferId });

export const setTransferPriority = (
  transferId: number,
  priority: TransferPriority,
) => invoke<void>("set_transfer_priority", { transferId, priority });

export const setMaxConcurrentTransfers = (value: number) =>
  invoke<number>("set_max_concurrent_transfers", { value });

export const clearFinishedTransfers = () =>
  invoke<number>("clear_finished_transfers");

/* ── Tunnels (port forwarding) ───────────────────────────────────────── */

export const openTunnel = (
  hostId: string,
  localPort: number,
  remoteHost: string,
  remotePort: number,
) =>
  invoke<TunnelInfo>("open_tunnel", { hostId, localPort, remoteHost, remotePort });

export const openRemoteTunnel = (
  hostId: string,
  remotePort: number,
  remoteBindHost: string,
  localHost: string,
  localPort: number,
) =>
  invoke<TunnelInfo>("open_remote_tunnel", {
    hostId,
    remotePort,
    remoteBindHost,
    localHost,
    localPort,
  });

export const closeTunnel = (hostId: string, tunnelId: number) =>
  invoke<void>("close_tunnel", { hostId, tunnelId });

export const listTunnels = (hostId: string) =>
  invoke<TunnelInfo[]>("list_tunnels", { hostId });

export function onTunnelEvent(
  handler: (event: TunnelEvent) => void,
): Promise<UnlistenFn> {
  return listen<TunnelEvent>("tunnel://state", ({ payload }) =>
    handler(payload),
  );
}

/** Byte-level progress for one transfer. Unlike the terminal and stream events
 *  these are broadcast, not addressed to a webview: the Transfers page is
 *  global and must keep updating while the user is anywhere in the app. */
export function onTransferProgress(
  handler: (event: TransferProgress) => void,
): Promise<UnlistenFn> {
  return listen<TransferProgress>("sftp://progress", ({ payload }) =>
    handler(payload),
  );
}

/** The queue's shape changed - something was added, promoted, re-ranked or
 *  settled. Carries no payload; the listener re-reads the list. */
export function onTransfersChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("sftp://changed", () => handler());
}

/* ── Errors ────────────────────────────────────────────────────────────── */

/** Rust errors arrive as plain strings; anything else is a bug worth showing. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}

/** Whether a failure was an unrecognised host key, so the UI can offer "trust
 *  and connect". A *changed* key is deliberately not matched - that one must
 *  never be click-through. */
export function isUnknownHostKey(error: unknown): boolean {
  return errorMessage(error).includes("HOSTKEY:unknown");
}

/** Pull the fingerprint out of a host key error for the confirmation dialog. */
export function hostKeyFingerprint(error: unknown): string | null {
  return errorMessage(error).match(/SHA256:[A-Za-z0-9+/=]+/)?.[0] ?? null;
}
