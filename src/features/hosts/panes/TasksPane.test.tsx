import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { connectionInfo } from "../../../test/connection";
import { TASK_BLOCKING_STORAGE_KEY, isBlocked } from "../../settings/preferences";
import type { DangerLevel, HostTasks, TaskPlan } from "../types";
import { TasksPane } from "./TasksPane";

vi.mock("../HostsProvider", () => ({ useHosts: () => ({ getConnection: () => connectionInfo() }) }));
vi.mock("../ElevationProvider", () => ({ useElevation: () => vi.fn() }));
vi.mock("../../../lib/terminalClipboard", () => ({ bindTerminalClipboard: () => () => {} }));
vi.mock("../../../theme/ThemeProvider", () => ({ useTheme: () => ({ resolved: "dark" }) }));

const catalog: HostTasks = {
  os: "linux",
  builtin: [{ id: "wipe", name: "Wipe", description: "", command: "rm -rf /", elevated: false, acts: true }],
  saved: [],
};

const plan = (level: DangerLevel): TaskPlan => ({
  command: "rm -rf /",
  innerCommand: "rm -rf /",
  elevated: false,
  needsPassword: false,
  wrapper: null,
  danger: { level, reasons: level === "none" ? [] : [{ label: "Recursive delete", detail: "Gone.", level }] },
});

let level: DangerLevel;

beforeEach(() => {
  localStorage.clear();
  level = "destructive";
  mockIPC((cmd) => {
    if (cmd === "list_host_tasks") return catalog;
    if (cmd === "plan_task") return plan(level);
  });
});

async function openDialog() {
  const user = userEvent.setup();
  render(<TasksPane hostId="h1" />);
  await user.click(await screen.findByRole("button", { name: "Run" }));
  return { user, dialog: await screen.findByRole("dialog") };
}

describe("blocking dangerous tasks", () => {
  it("blocks a destructive task by default", async () => {
    const { dialog } = await openDialog();
    expect(within(dialog).getByText(/Blocked by your settings/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Run" })).toBeDisabled();
    expect(within(dialog).queryByLabelText(/Type/)).not.toBeInTheDocument();
  });

  it("falls back to the typed confirmation when turned off", async () => {
    localStorage.setItem(TASK_BLOCKING_STORAGE_KEY, "off");
    const { dialog } = await openDialog();
    expect(within(dialog).queryByText(/Blocked by your settings/)).not.toBeInTheDocument();
    expect(within(dialog).getByLabelText(/Type/)).toBeInTheDocument();
  });

  it("lets a caution through by default", async () => {
    level = "caution";
    const first = await openDialog();
    expect(within(first.dialog).getByRole("button", { name: "Run" })).toBeEnabled();
  });

  it("maps each setting to the levels it stops", () => {
    expect(isBlocked("destructive", "destructive")).toBe(true);
    expect(isBlocked("caution", "destructive")).toBe(false);
    expect(isBlocked("caution", "caution")).toBe(true);
    expect(isBlocked("none", "caution")).toBe(false);
    expect(isBlocked("destructive", "off")).toBe(false);
  });
});
