import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TransferRecord, TransferSummary } from "../hosts/types";

vi.mock("../../lib/toast", () => ({ success: vi.fn(), error: vi.fn() }));

const record = (overrides: Partial<TransferRecord>): TransferRecord => ({
  id: 1,
  hostId: "h",
  hostLabel: "web-1",
  direction: "download",
  remotePath: "/srv/a.tar",
  localPath: "/tmp/a.tar",
  name: "a.tar",
  priority: "normal",
  state: "running",
  bytesDone: 0,
  bytesTotal: 1000,
  queuePosition: null,
  error: null,
  queuedAt: "2026-09-17T12:00:00Z",
  startedAt: null,
  finishedAt: null,
  ...overrides,
});

let backend: { records: TransferRecord[]; summary: TransferSummary };

async function load() {
  vi.resetModules();
  const store = await import("./transferStore");
  const toast = await import("../../lib/toast");
  return { store, toast };
}

/** Let mocked IPC promises and event listeners settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  backend = {
    records: [],
    summary: { running: 0, queued: 0, maxConcurrent: 3 },
  };
  mockIPC(
    (cmd) => {
      if (cmd === "list_transfers") return backend.records;
      if (cmd === "transfer_summary") return backend.summary;
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => vi.useRealTimers());

describe("refresh", () => {
  it("mirrors the backend list and counts", async () => {
    const { store } = await load();
    backend.records = [record({ id: 1 }), record({ id: 2, hostId: "other" })];
    backend.summary = { running: 1, queued: 1, maxConcurrent: 3 };

    await store.refresh();

    expect(store.all()).toHaveLength(2);
    expect(store.forHost("h").map((r) => r.id)).toEqual([1]);
    expect(store.pendingCount()).toBe(2);
    expect(store.activeCount()).toBe(1);
  });

  it("keeps the last list when a read fails", async () => {
    const { store } = await load();
    backend.records = [record({ id: 1 })];
    await store.refresh();

    mockIPC(() => {
      throw new Error("backend gone");
    });
    await store.refresh();
    expect(store.all()).toHaveLength(1);
  });
});

describe("settled announcements", () => {
  it("stays quiet on the first load", async () => {
    const { store, toast } = await load();
    backend.records = [record({ state: "done" })];
    await store.refresh();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("toasts a transition to done or failed exactly once", async () => {
    const { store, toast } = await load();
    backend.records = [record({ id: 1 }), record({ id: 2, direction: "upload", name: "b" })];
    await store.refresh();

    backend.records = [
      record({ id: 1, state: "done" }),
      record({ id: 2, direction: "upload", name: "b", state: "failed", error: "disk full" }),
    ];
    await store.refresh();
    await store.refresh();

    expect(toast.success).toHaveBeenCalledTimes(1);
    expect(toast.success).toHaveBeenCalledWith("Downloaded a.tar", "web-1");
    expect(toast.error).toHaveBeenCalledTimes(1);
    expect(toast.error).toHaveBeenCalledWith("Upload failed: b", "disk full");
  });

  it("does not announce a cancellation", async () => {
    const { store, toast } = await load();
    backend.records = [record({})];
    await store.refresh();
    backend.records = [record({ state: "canceled" })];
    await store.refresh();
    expect(toast.success).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });
});

describe("progress and rate", () => {
  const progress = (bytesDone: number) =>
    emit("sftp://progress", {
      transferId: 1,
      hostId: "h",
      bytesDone,
      bytesTotal: 1000,
      state: "running",
    });

  it("patches bytes and derives a smoothed rate", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(0);
    const { store } = await load();
    backend.records = [record({})];
    store.start();
    await settle();

    await progress(0);
    await settle();
    expect(store.rateOf(1)).toBeNull();

    // Too soon after the last sample to divide by.
    vi.setSystemTime(100);
    await progress(50);
    await settle();
    expect(store.rateOf(1)).toBeNull();
    expect(store.get(1)?.bytesDone).toBe(50);

    vi.setSystemTime(1000);
    await progress(1000);
    await settle();
    expect(store.rateOf(1)).toBe(1000);

    vi.setSystemTime(2000);
    await progress(1000);
    await settle();
    expect(store.rateOf(1)).toBe(600);
  });

  it("drops the rate once a transfer stops running", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(0);
    const { store } = await load();
    backend.records = [record({})];
    store.start();
    await settle();

    await progress(0);
    vi.setSystemTime(1000);
    await progress(500);
    await settle();
    expect(store.rateOf(1)).toBe(500);

    backend.records = [record({ state: "done" })];
    await emit("sftp://changed");
    await settle();
    expect(store.rateOf(1)).toBeNull();
  });
});
