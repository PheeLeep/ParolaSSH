/** The Rust command surface for saved connections and live sessions.
 *
 *  Passwords are passed as arguments and never returned. The Rust side holds
 *  them in memory for the app's lifetime at most — nothing here writes a
 *  secret to disk, and there is no command that reads one back. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ConnectionInfo,
  HostDraft,
  HostHealth,
  HostMetrics,
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

/* ── Reaching them ─────────────────────────────────────────────────────── */

/** Is anything listening on that port, and does it speak SSH? */
export const probeHost = (hostname: string, port: number) =>
  invoke<ProbeResult>("probe_host", { hostname, port });

/**
 * Connect and authenticate.
 *
 * `trustUnknown` records an unrecognised host key — only pass it once the
 * user has actually seen and accepted the fingerprint.
 */
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

export const disconnectHost = (hostId: string) =>
  invoke<boolean>("disconnect_host", { hostId });

export const connectedHosts = () => invoke<string[]>("connected_hosts");

export const hasRememberedPassword = (hostId: string) =>
  invoke<boolean>("has_remembered_password", { hostId });

export const forgetPassword = (hostId: string) =>
  invoke<void>("forget_password", { hostId });

/**
 * Check every saved host: are they up, and are our sessions still good?
 *
 * Connected hosts get a round trip on the existing session; the rest get a
 * TCP probe. Sessions that fail are dropped by the Rust side, so the result
 * is authoritative — a host reported as disconnected really is.
 */
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

/**
 * Open a shell and return its id.
 *
 * Opening does not replace anything — a host can hold several terminals on the
 * one authenticated connection. The id addresses every later call and tags
 * every event.
 */
export const openShell = (hostId: string, cols: number, rows: number) =>
  invoke<number>("open_shell", { hostId, cols, rows });

/** Shell ids already open on a host, so a reload can rebuild its tabs. */
export const listShells = (hostId: string) =>
  invoke<number[]>("list_shells", { hostId });

export const writeShell = (hostId: string, shellId: number, data: string) =>
  invoke<void>("write_shell", { hostId, shellId, data });

export const resizeShell = (
  hostId: string,
  shellId: number,
  cols: number,
  rows: number,
) => invoke<void>("resize_shell", { hostId, shellId, cols, rows });

/** Close one shell. An id that has already gone is a no-op, not an error. */
export const closeShell = (hostId: string, shellId: number) =>
  invoke<void>("close_shell", { hostId, shellId });

/**
 * Subscribe to terminal output for one host.
 *
 * `isMine` decides which shell's bytes to render. Filtering on host id alone
 * is not enough: a shell that has been replaced keeps streaming until it winds
 * down, and its banner and prompt would appear in the new pane alongside the
 * real ones — which reads as duplicated output.
 */
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

/**
 * The last journal lines (Linux) or SCM events (Windows) for a service.
 * `displayName` matters only on Windows, where events name services by their
 * display name.
 */
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

/** Read-only, always — there is no install command to call. */
export const checkUpdates = (hostId: string) =>
  invoke<UpdateReport>("check_updates", { hostId });

/* ── Remote audit ──────────────────────────────────────────────────────── */

/** Tiers 0–1 plus Lynis detection. `password` feeds the sudo retry only. */
export const remoteAudit = (hostId: string, password?: string | null) =>
  invoke<RemoteAuditReport>("remote_audit", { hostId, password: password ?? null });

export const setRemoteFindingSuppressed = (
  hostId: string,
  findingId: string,
  suppressed: boolean,
) => invoke<void>("set_remote_finding_suppressed", { hostId, findingId, suppressed });

/* ── Errors ────────────────────────────────────────────────────────────── */

/** Rust errors arrive as plain strings; anything else is a bug worth showing. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}

/**
 * Whether a failure was an unrecognised host key.
 *
 * The Rust side tags that message so the UI can offer "trust and connect"
 * instead of a dead end. A *changed* key is deliberately not matched here —
 * that one should never be click-through.
 */
export function isUnknownHostKey(error: unknown): boolean {
  return errorMessage(error).includes("HOSTKEY:unknown");
}

/** Pull the fingerprint out of a host key error for the confirmation dialog. */
export function hostKeyFingerprint(error: unknown): string | null {
  return errorMessage(error).match(/SHA256:[A-Za-z0-9+/=]+/)?.[0] ?? null;
}
