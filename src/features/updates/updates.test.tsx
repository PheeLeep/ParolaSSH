import { mockIPC } from "@tauri-apps/api/mocks";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const check = vi.fn();
const relaunch = vi.fn();
const openExternal = vi.fn();
let connectedCount = 0;

vi.mock("@tauri-apps/plugin-updater", () => ({ check: () => check() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: () => relaunch() }));
vi.mock("../../lib/openExternal", () => ({ openExternal: (url: string) => openExternal(url) }));
vi.mock("../hosts/HostsProvider", () => ({ useHosts: () => ({ connectedCount }) }));

import { UpdateBanner } from "./UpdateBanner";
import * as updates from "./updateStore";

let kind = "updatable";
const downloadAndInstall = vi.fn(async () => {});
const release = { version: "1.0.1", body: "Fixes", downloadAndInstall };

beforeEach(() => {
  updates.reset();
  kind = "updatable";
  connectedCount = 0;
  check.mockReset();
  relaunch.mockReset();
  openExternal.mockReset();
  downloadAndInstall.mockClear();
  mockIPC((cmd) => (cmd === "install_kind" ? kind : undefined));
});

describe("update store", () => {
  it("reports an available release", async () => {
    check.mockResolvedValue(release);
    await updates.checkForUpdate();
    expect(updates.getState()).toMatchObject({ status: "available", version: "1.0.1", kind: "updatable" });
    expect(updates.bannerVisible()).toBe(true);
  });

  it("says when this is the latest version", async () => {
    check.mockResolvedValue(null);
    await updates.checkForUpdate();
    expect(updates.getState().status).toBe("current");
  });

  it("keeps a failed startup check quiet but shows a manual one", async () => {
    check.mockRejectedValue(new Error("offline"));
    await updates.checkForUpdate({ quiet: true });
    expect(updates.getState().status).toBe("idle");
    await updates.checkForUpdate();
    expect(updates.getState()).toMatchObject({ status: "error", message: "offline" });
  });

  it("never asks GitHub from a development build", async () => {
    kind = "unsupported";
    await updates.checkForUpdate({ quiet: true });
    expect(check).not.toHaveBeenCalled();
  });

  it("hides a dismissed version", async () => {
    check.mockResolvedValue(release);
    await updates.checkForUpdate();
    updates.dismiss();
    expect(updates.bannerVisible()).toBe(false);
  });
});

describe("UpdateBanner", () => {
  it("installs and restarts when nothing is connected", async () => {
    check.mockResolvedValue(release);
    await updates.checkForUpdate();
    render(<UpdateBanner />);

    await userEvent.setup().click(screen.getByRole("button", { name: /install and restart/i }));
    expect(downloadAndInstall).toHaveBeenCalled();
    expect(relaunch).toHaveBeenCalled();
  });

  it("warns before closing open sessions", async () => {
    connectedCount = 2;
    check.mockResolvedValue(release);
    await updates.checkForUpdate();
    render(<UpdateBanner />);
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: /install and restart/i }));
    expect(screen.getByText("Restarting closes 2 open sessions.")).toBeInTheDocument();
    expect(downloadAndInstall).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /install anyway/i }));
    expect(downloadAndInstall).toHaveBeenCalled();
  });

  it("sends a .deb install to the download page instead", async () => {
    kind = "package";
    check.mockResolvedValue(release);
    await updates.checkForUpdate();
    render(<UpdateBanner />);

    expect(screen.queryByRole("button", { name: /install/i })).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Download" }));
    expect(openExternal).toHaveBeenCalledWith(updates.RELEASES_URL);
  });

  it("goes away on Later", async () => {
    check.mockResolvedValue(release);
    await updates.checkForUpdate();
    render(<UpdateBanner />);

    await userEvent.setup().click(screen.getByRole("button", { name: "Later" }));
    expect(screen.queryByText(/is available/)).not.toBeInTheDocument();
  });

  it("renders nothing without an update", () => {
    act(() => {
      render(<UpdateBanner />);
    });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
