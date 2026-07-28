/** Every file transfer in the app, in one place.
 *
 *  A module singleton rather than a React provider, for the same reason
 *  `terminalStore` is one: a transfer starts in a host's Files pane and must
 *  keep running while the user navigates anywhere else, including away from
 *  that host entirely. State that lives in the tree dies with the tree.
 *
 *  Rust owns the queue - ordering, the concurrency cap, what runs next. This
 *  is a cache of it, refreshed from `list_transfers` whenever the backend says
 *  the shape changed, with byte counts patched in between from the progress
 *  events. The split matters: two windows, or a reload, must never disagree
 *  about what is running.
 */

import type { UnlistenFn } from "@tauri-apps/api/event";

import * as api from "../hosts/api";
import * as toast from "../../lib/toast";
import type {
  TransferPriority,
  TransferRecord,
  TransferSummary,
} from "../hosts/types";

let records: TransferRecord[] = [];
let summary: TransferSummary = { running: 0, queued: 0, maxConcurrent: 3 };

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

/** Every transfer, newest first - the order Rust returns them in. */
export function all(): TransferRecord[] {
  return records;
}

export function forHost(hostId: string): TransferRecord[] {
  return records.filter((record) => record.hostId === hostId);
}

export function get(id: number): TransferRecord | undefined {
  return records.find((record) => record.id === id);
}

export function counts(): TransferSummary {
  return summary;
}

/** Running plus queued - what the sidebar badge shows. */
export function pendingCount(): number {
  return summary.running + summary.queued;
}

/** Transfers still moving bytes, for the close guard. */
export function activeCount(): number {
  return summary.running;
}

/** Re-read the whole list from Rust, which is the authority on ordering. */
export async function refresh(): Promise<void> {
  try {
    const [next, nextSummary] = await Promise.all([
      api.listTransfers(),
      api.transferSummary(),
    ]);
    announceSettled(records, next);
    pruneSamples(next);
    records = next;
    summary = nextSummary;
    emit();
  } catch {
    // The backend is the source of truth; a failed read just leaves the last
    // known list on screen rather than blanking it.
  }
}

/** Toast transfers that finished since the last read.
 *
 *  The whole point of a background queue is that you stop watching it, so an
 *  outcome nobody is looking at still has to reach the user. Only transitions
 *  are announced - re-reading the same finished row must not toast twice.
 */
function announceSettled(before: TransferRecord[], after: TransferRecord[]) {
  if (before.length === 0) return; // First load is history, not news.

  const previous = new Map(before.map((record) => [record.id, record.state]));

  for (const record of after) {
    const was = previous.get(record.id);
    if (was === undefined || was === record.state) continue;
    if (was !== "queued" && was !== "running") continue;

    const direction = record.direction === "download" ? "Downloaded" : "Uploaded";
    if (record.state === "done") {
      toast.success(`${direction} ${record.name}`, record.hostLabel);
    } else if (record.state === "failed") {
      toast.error(
        `${record.direction === "download" ? "Download" : "Upload"} failed: ${record.name}`,
        record.error ?? undefined,
      );
    }
    // A cancellation was the user's own doing and needs no announcement.
  }
}

/** Patch one row's byte counts without a round trip.
 *
 *  Progress arrives far more often than the queue changes shape, and refetching
 *  the list on every chunk would put a command round trip in the path of a
 *  progress bar. Anything structural still comes from `refresh`. */
function applyProgress(id: number, bytesDone: number, bytesTotal: number | null) {
  sampleRate(id, bytesDone);
  let changed = false;
  records = records.map((record) => {
    if (record.id !== id) return record;
    changed = true;
    return {
      ...record,
      bytesDone,
      bytesTotal: bytesTotal ?? record.bytesTotal,
    };
  });
  if (changed) emit();
}

/* ── Speed ─────────────────────────────────────────────────────────────── */

/** Last progress sample per transfer, plus the smoothed rate derived from it. */
type Sample = { bytes: number; at: number; rate: number | null };

const samples = new Map<number, Sample>();

/** Bytes per second for a running transfer, or null before there are two
 *  samples far enough apart to divide by. Rust reports bytes, not speed -
 *  timing them here keeps the backend out of the business of guessing what
 *  window a progress bar wants. */
export function rateOf(id: number): number | null {
  return samples.get(id)?.rate ?? null;
}

/** Fold a progress sample into the rate. Chunks land irregularly, so short
 *  gaps are skipped rather than divided by, and an EMA keeps the number
 *  readable instead of flickering with every chunk. */
function sampleRate(id: number, bytesDone: number) {
  const now = Date.now();
  const previous = samples.get(id);

  if (!previous) {
    samples.set(id, { bytes: bytesDone, at: now, rate: null });
    return;
  }

  const elapsed = now - previous.at;
  if (elapsed < 500) return;

  const instant = Math.max(0, ((bytesDone - previous.bytes) * 1000) / elapsed);
  samples.set(id, {
    bytes: bytesDone,
    at: now,
    rate: previous.rate === null ? instant : previous.rate * 0.6 + instant * 0.4,
  });
}

/** Drop samples for anything no longer moving, so a finished row shows no
 *  speed and the map does not grow with the history list. */
function pruneSamples(current: TransferRecord[]) {
  const running = new Set(
    current.filter((record) => record.state === "running").map((record) => record.id),
  );
  for (const id of samples.keys()) {
    if (!running.has(id)) samples.delete(id);
  }
}

let wiring: Promise<UnlistenFn[]> | null = null;

/** Attach to the backend's events. Idempotent - the first caller wires it and
 *  everyone after shares that subscription, so mounting two panes does not
 *  double-count events. */
export function start(): void {
  if (wiring) return;

  wiring = (async () => {
    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await api.onTransferProgress(({ transferId, bytesDone, bytesTotal }) => {
        applyProgress(transferId, bytesDone, bytesTotal);
      }),
    );
    unlisteners.push(await api.onTransfersChanged(() => void refresh()));
    await refresh();
    return unlisteners;
  })();
}

export function cancel(id: number): Promise<void> {
  return api.cancelTransfer(id).then(refresh);
}

export function prioritize(id: number, priority: TransferPriority): Promise<void> {
  return api.setTransferPriority(id, priority).then(refresh);
}

export function clearFinished(): Promise<void> {
  return api.clearFinishedTransfers().then(() => refresh());
}
