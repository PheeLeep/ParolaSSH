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
let saved: Record<string, unknown> | null;

beforeEach(() => {
  localStorage.clear();
  level = "destructive";
  saved = null;
  mockIPC((cmd, args) => {
    if (cmd === "list_host_tasks") return catalog;
    if (cmd === "plan_task") return plan(level);
    if (cmd === "assess_task_command") return plan(level).danger;
    if (cmd === "save_task") {
      saved = args as Record<string, unknown>;
      return { id: "t-1" };
    }
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
    // One alert, carrying both the verdict and the reason.
    expect(within(dialog).getAllByRole("alert")).toHaveLength(1);
    expect(within(dialog).getByText("Blocked: this command cannot be run")).toBeInTheDocument();
    expect(within(dialog).getByText(/Recursive delete/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Run" })).toBeDisabled();
    expect(within(dialog).queryByLabelText(/Type/)).not.toBeInTheDocument();
  });

  it("falls back to the typed confirmation when turned off", async () => {
    localStorage.setItem(TASK_BLOCKING_STORAGE_KEY, "off");
    const { dialog } = await openDialog();
    expect(within(dialog).queryByText(/Blocked:/)).not.toBeInTheDocument();
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

describe("the task editor", () => {
  async function openEditor() {
    const user = userEvent.setup();
    render(<TasksPane hostId="h1" />);
    await user.click(await screen.findByRole("button", { name: /new task/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText("Name"), "Nuke");
    await user.type(within(dialog).getByLabelText("Command"), "rm -rf /");
    return { user, dialog };
  }

  it("will not save a command the setting blocks", async () => {
    const { dialog } = await openEditor();
    expect(await within(dialog).findByText("Blocked: this command cannot be saved")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("saves a host-only task with its host id", async () => {
    level = "none";
    const { user, dialog } = await openEditor();
    await user.click(within(dialog).getByLabelText("Available on every host"));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(saved).toMatchObject({
      draft: { scope: { kind: "host", hostId: "h1" } },
      hostId: "h1",
      blockFrom: "destructive",
    });
  });
});
