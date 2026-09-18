import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { connectionInfo } from "../../test/connection";
import { hostRow } from "../../test/fixtures";
import type { ConnectionInfo } from "./types";
import { PowerDialog } from "./PowerDialog";

let connection: ConnectionInfo;
vi.mock("./HostsProvider", () => ({ useHosts: () => ({ getConnection: () => connection, power: vi.fn() }) }));
vi.mock("./ElevationProvider", () => ({ useElevation: () => vi.fn() }));

beforeEach(() => {
  connection = connectionInfo();
  mockIPC((cmd) => {
    if (cmd === "preview_power") return { command: "shutdown -r now", needsPassword: false, summary: "Reboot immediately" };
  });
});

describe("PowerDialog", () => {
  it("offers scheduling on a full system", async () => {
    render(<PowerDialog host={hostRow()} onClose={vi.fn()} />);
    expect(await screen.findByText("shutdown -r now")).toBeInTheDocument();
    expect(screen.getByText("When")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Call off a scheduled shutdown/ })).toBeInTheDocument();
  });

  it("explains a container that cannot power itself and offers nothing to run", () => {
    connection = connectionInfo({
      powerRefusal: "This is a Docker container whose main process is `sshd`, not an init system.",
      platform: { init: "sysv", container: "docker", pid1: "sshd", hasShutdown: true },
    });
    render(<PowerDialog host={hostRow()} onClose={vi.fn()} />);

    expect(screen.getByText(/Docker container whose main process/)).toBeInTheDocument();
    expect(screen.queryByText("Action")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
  });

  it("drops scheduling and cancel where there is no shutdown command", async () => {
    connection = connectionInfo({
      supportsDelay: false,
      supportsCancel: false,
      platform: { init: "openrc", container: null, pid1: "init", hasShutdown: false },
    });
    render(<PowerDialog host={hostRow()} onClose={vi.fn()} />);

    expect(screen.getByText(/can only reboot or power off immediately/)).toBeInTheDocument();
    expect(screen.queryByText("When")).not.toBeInTheDocument();
    expect(screen.queryByText("Message to logged-in users")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Call off a scheduled shutdown/ })).not.toBeInTheDocument();
  });
});
