import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { connectionInfo } from "../../../test/connection";
import type { ConnectionInfo, ServiceEntry } from "../types";
import { ServicesPane } from "./ServicesPane";

let connection: ConnectionInfo;
vi.mock("../HostsProvider", () => ({ useHosts: () => ({ getConnection: () => connection }) }));
vi.mock("../ElevationProvider", () => ({ useElevation: () => vi.fn() }));

const ssh: ServiceEntry = { name: "ssh", description: "", state: "running", detail: "running" };

describe("ServicesPane", () => {
  beforeEach(() => {
    connection = connectionInfo();
  });

  it("warns that PID 1's service carries the whole container", async () => {
    connection = connectionInfo({ platform: { init: "sysv", container: "docker", pid1: "sshd", hasShutdown: true } });
    mockIPC((cmd) => (cmd === "list_services" ? [ssh] : undefined));
    render(<ServicesPane hostId="h1" />);

    expect(await screen.findByText("ssh")).toBeInTheDocument();
    const warning = screen.getByText(/stops the whole container/);
    expect(warning).toHaveTextContent("Docker container whose main process is sshd");
  });

  it("says nothing about containers on a full system", async () => {
    mockIPC((cmd) => (cmd === "list_services" ? [ssh] : undefined));
    render(<ServicesPane hostId="h1" />);
    await screen.findByText("ssh");
    expect(screen.queryByText(/whole container/)).not.toBeInTheDocument();
  });

  it("shows a host without a service manager as information, not an error", async () => {
    connection = connectionInfo({ platform: { init: "none", container: "docker", pid1: "node", hasShutdown: false } });
    mockIPC((cmd) => {
      if (cmd === "list_services") throw "This is a Docker container with no service manager.";
    });
    render(<ServicesPane hostId="h1" />);

    const notice = await screen.findByText(/no service manager/);
    expect(notice.closest(".alert")).toHaveClass("alert-secondary");
  });
});
