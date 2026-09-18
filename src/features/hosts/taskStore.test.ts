import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// xterm needs a real canvas; the store only needs something to write into.
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown> = {};
    loadAddon() {}
    open() {}
    write() {}
    writeln() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));
vi.mock("../../lib/terminalClipboard", () => ({ bindTerminalClipboard: () => () => {} }));

import * as store from "./taskStore";
import type { TaskPlan } from "./types";

const plan: TaskPlan = {
  command: "uptime",
  innerCommand: "uptime",
  elevated: false,
  needsPassword: false,
  wrapper: null,
  danger: { level: "none", reasons: [] },
};

let nextStream = 1;

// Before the shared setup clears the IPC mock, which the unlisteners still need.
afterEach(async () => {
  await store.closeHost("h");
});

beforeEach(() => {
  nextStream = 1;
  mockIPC(
    (cmd) => {
      if (cmd === "start_task") return nextStream++;
      return null;
    },
    { shouldMockEvents: true },
  );
});

const run = (taskId: string) => store.start("h", taskId, taskId.toUpperCase(), plan, "dark");
const finish = (streamId: number) => emit("stream://closed", { hostId: "h", streamId, exitCode: 0 });
const ids = () => ["disk", "ports"].filter((id) => store.find("h", id));

describe("task results", () => {
  it("keeps one run per task", async () => {
    await run("disk");
    await run("ports");
    expect(ids()).toEqual(["disk", "ports"]);
    expect(store.find("h", "disk")?.state).toBe("running");
  });

  it("refuses to start a task that is still running", async () => {
    await run("disk");
    await expect(run("disk")).rejects.toThrow(/still running/);
  });

  it("replaces a finished run with the rerun", async () => {
    await run("disk");
    await finish(1);
    const first = store.find("h", "disk");
    expect(first?.state).toBe("finished");

    await run("disk");
    const second = store.find("h", "disk");
    expect(second).not.toBe(first);
    expect(second?.state).toBe("running");
  });

  it("drops everything when the host goes", async () => {
    await run("disk");
    await store.closeHost("h");
    expect(store.find("h", "disk")).toBeUndefined();
  });
});
