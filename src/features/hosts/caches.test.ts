import { beforeEach, describe, expect, it, vi } from "vitest";

import type { HostMetrics, RemoteAuditReport } from "./types";

async function load() {
  vi.resetModules();
  return {
    audit: await import("./auditCache"),
    metrics: await import("./metricsCache"),
  };
}

let caches: Awaited<ReturnType<typeof load>>;
beforeEach(async () => {
  caches = await load();
});

describe("auditCache", () => {
  const report = {} as RemoteAuditReport;

  it("only lets the first run per host through", () => {
    expect(caches.audit.markAttempted("h")).toBe(true);
    expect(caches.audit.markAttempted("h")).toBe(false);
    expect(caches.audit.markAttempted("other")).toBe(true);
  });

  it("forget clears the report and allows another run", () => {
    caches.audit.markAttempted("h");
    caches.audit.set("h", report);
    caches.audit.forget("h");
    expect(caches.audit.get("h")).toBeUndefined();
    expect(caches.audit.markAttempted("h")).toBe(true);
  });

  it("notifies only on real changes", () => {
    const listener = vi.fn();
    caches.audit.subscribe(listener);
    caches.audit.clear("h");
    expect(listener).not.toHaveBeenCalled();
    caches.audit.set("h", report);
    caches.audit.clear("h");
    expect(listener).toHaveBeenCalledTimes(2);
  });
});

describe("metricsCache", () => {
  const sample = (n: number) => ({ n }) as unknown as HostMetrics;

  it("keeps only the newest samples", () => {
    for (let i = 0; i < caches.metrics.HISTORY_LIMIT + 5; i++) {
      caches.metrics.push("h", sample(i));
    }
    const history = caches.metrics.history("h");
    expect(history).toHaveLength(caches.metrics.HISTORY_LIMIT);
    expect(history[0]).toEqual(sample(5));
  });

  it("keeps history when the interval changes, and drops both on forget", () => {
    caches.metrics.push("h", sample(1));
    caches.metrics.setInterval("h", "5");
    expect(caches.metrics.interval("h")).toBe("5");
    expect(caches.metrics.history("h")).toHaveLength(1);

    caches.metrics.forget("h");
    expect(caches.metrics.history("h")).toEqual([]);
    expect(caches.metrics.interval("h")).toBe("1");
  });
});
