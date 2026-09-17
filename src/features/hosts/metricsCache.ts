/**
 * Performance history per host, kept while the session lives.
 *
 * The Performance pane unmounts on every tab switch, so history held in its
 * state would start over each time. It lives here instead and is dropped on
 * the same events the audit report is: disconnect, reap, power-off, delete.
 */

import type { HostMetrics } from "./types";

/** How many samples the charts keep. */
export const HISTORY_LIMIT = 60;

export type IntervalChoice = "1" | "2" | "5" | "10" | "30";

type Entry = {
  history: HostMetrics[];
  interval: IntervalChoice;
};

const DEFAULT_INTERVAL: IntervalChoice = "1";

const entries = new Map<string, Entry>();
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function history(hostId: string): HostMetrics[] {
  return entries.get(hostId)?.history ?? [];
}

export function interval(hostId: string): IntervalChoice {
  return entries.get(hostId)?.interval ?? DEFAULT_INTERVAL;
}

export function push(hostId: string, sample: HostMetrics): void {
  const entry = entries.get(hostId);
  const next = [...(entry?.history ?? []), sample].slice(-HISTORY_LIMIT);
  entries.set(hostId, { history: next, interval: entry?.interval ?? DEFAULT_INTERVAL });
  emit();
}

export function setInterval(hostId: string, choice: IntervalChoice): void {
  entries.set(hostId, { history: history(hostId), interval: choice });
  emit();
}

export function forget(hostId: string): void {
  if (entries.delete(hostId)) emit();
}
