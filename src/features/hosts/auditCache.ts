/**
 * The last posture report per host, held for as long as the session is.
 *
 * The Audit pane used to own its report in component state, which made "run the
 * checks when a host connects" impossible to honour: nothing runs until the
 * pane is opened, and the pane is opened long after the connection. The report
 * lives here instead, so the run can happen at connect time and the pane simply
 * shows what is already there.
 *
 * Cleared on the same events a terminal is — the report
 * describes a live session, and a stale one under a reconnected host would be
 * a lie about the machine as it is now.
 */

import type { RemoteAuditReport } from "./types";

const reports = new Map<string, RemoteAuditReport>();
const listeners = new Set<() => void>();

let version = 0;

function emit() {
  version += 1;
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getVersion(): number {
  return version;
}

export function get(hostId: string): RemoteAuditReport | undefined {
  return reports.get(hostId);
}

export function set(hostId: string, report: RemoteAuditReport): void {
  reports.set(hostId, report);
  emit();
}

export function clear(hostId: string): void {
  if (reports.delete(hostId)) emit();
}

/** True once a run has been started for this host, whether or not it finished.
 *  Keeps the connect-time run from firing a second time when the pane opens. */
const attempted = new Set<string>();

export function markAttempted(hostId: string): boolean {
  if (attempted.has(hostId)) return false;
  attempted.add(hostId);
  return true;
}

export function forget(hostId: string): void {
  attempted.delete(hostId);
  clear(hostId);
}
