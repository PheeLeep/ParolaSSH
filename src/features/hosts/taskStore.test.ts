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
const tabs = () => store.list("h").runs.map((entry) => entry.taskId);

describe("task tabs", () => {
  it("opens one tab per task and shows the newest", async () => {
    await run("disk");
    await run("ports");
    expect(tabs()).toEqual(["disk", "ports"]);
    expect(store.list("h").active?.taskId).toBe("ports");
  });

  it("switches to a task that is still running instead of starting it again", async () => {
    await run("disk");
    await run("ports");
    await expect(run("disk")).rejects.toThrow(/still running/);
    expect(tabs()).toEqual(["disk", "ports"]);
    expect(store.list("h").active?.taskId).toBe("disk");
  });

  it("reruns a finished task in its own tab", async () => {
    await run("disk");
    await run("ports");
    await finish(1);
    expect(store.find("h", "disk")?.state).toBe("finished");

    await run("disk");
    expect(tabs()).toEqual(["disk", "ports"]);
    expect(store.find("h", "disk")?.state).toBe("running");
    expect(store.list("h").active?.taskId).toBe("disk");
  });

  it("lands on the neighbouring tab when the shown one closes", async () => {
    await run("a");
    await run("b");
    await run("c");
    store.select("h", "b");
    await store.close("h", "b");
    expect(tabs()).toEqual(["a", "c"]);
    expect(store.list("h").active?.taskId).toBe("c");
  });

  it("caps the strip at the terminal's limit", async () => {
    for (let index = 0; index < store.MAX_TABS; index += 1) await run(`t${index}`);
    await expect(run("one-more")).rejects.toThrow(/tabs are open/);
  });
});
