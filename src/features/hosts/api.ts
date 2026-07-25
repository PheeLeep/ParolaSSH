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
  PowerOutcome,
  PowerPlan,
  PowerRequest,
  ProbeResult,
  SshHost,
  TerminalClosed,
  TerminalOutput,
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

export const openShell = (hostId: string, cols: number, rows: number) =>
  invoke<void>("open_shell", { hostId, cols, rows });

export const writeShell = (hostId: string, data: string) =>
  invoke<void>("write_shell", { hostId, data });

export const resizeShell = (hostId: string, cols: number, rows: number) =>
  invoke<void>("resize_shell", { hostId, cols, rows });

export const closeShell = (hostId: string) =>
  invoke<void>("close_shell", { hostId });

/**
 * Subscribe to terminal output for one host.
 *
 * The Rust side addresses these events to this window only, but every session
 * in the window shares one event name — so the host id is still filtered here,
 * or two open terminals would each render the other's bytes.
 */
export function onTerminalOutput(
  hostId: string,
  handler: (event: TerminalOutput) => void,
): Promise<UnlistenFn> {
  return listen<TerminalOutput>("terminal://output", ({ payload }) => {
    if (payload.hostId === hostId) handler(payload);
  });
}

export function onTerminalClosed(
  hostId: string,
  handler: (event: TerminalClosed) => void,
): Promise<UnlistenFn> {
  return listen<TerminalClosed>("terminal://closed", ({ payload }) => {
    if (payload.hostId === hostId) handler(payload);
  });
}

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
