import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { TunnelInfo } from "../types";
import { TunnelsPane } from "./TunnelsPane";

let tunnels: TunnelInfo[];
let calls: { cmd: string; args: Record<string, unknown> }[];
let failWith: string | null;

const tunnel = (overrides: Partial<TunnelInfo>): TunnelInfo => ({
  id: 1,
  hostId: "h",
  direction: "local",
  localPort: 15432,
  localHost: "127.0.0.1",
  remoteHost: "127.0.0.1",
  remotePort: 5432,
  activeConnections: 0,
  lastError: null,
  ...overrides,
});

beforeEach(() => {
  tunnels = [];
  calls = [];
  failWith = null;
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
      if (cmd === "list_tunnels") return tunnels;
      if (cmd === "open_tunnel" || cmd === "open_remote_tunnel") {
        if (failWith) throw failWith;
        return tunnel({});
      }
      if (cmd === "close_tunnel") {
        tunnels = [];
        return null;
      }
    },
    { shouldMockEvents: true },
  );
});

const opened = (cmd: string) => calls.find((call) => call.cmd === cmd)?.args;

async function openForm(direction: "Local (-L)" | "Remote (-R)") {
  const user = userEvent.setup();
  render(<TunnelsPane hostId="h" />);
  await user.click(await screen.findByRole("button", { name: /new tunnel/i }));
  if (direction === "Remote (-R)") {
    await user.click(screen.getByRole("button", { name: "Remote (-R)" }));
  }
  const open = screen.getByRole("button", { name: "Open" });
  return { user, open };
}

describe("TunnelsPane", () => {
  it("explains the empty state", async () => {
    render(<TunnelsPane hostId="h" />);
    expect(await screen.findByText(/no active tunnels/i)).toBeInTheDocument();
  });

  it("opens a local tunnel with an automatic local port", async () => {
    const { user, open } = await openForm("Local (-L)");
    expect(open).toBeDisabled();

    await user.type(screen.getByPlaceholderText("e.g. 5432"), "5432");
    await user.click(open);

    await waitFor(() =>
      expect(opened("open_tunnel")).toEqual({
        hostId: "h",
        localPort: 0,
        remoteHost: "127.0.0.1",
        remotePort: 5432,
      }),
    );
  });

  it("requires a local target port for a remote tunnel", async () => {
    const { user, open } = await openForm("Remote (-R)");
    expect(open).toBeDisabled();

    await user.type(screen.getByPlaceholderText("e.g. 3000"), "0");
    expect(open).toBeDisabled();

    await user.clear(screen.getByPlaceholderText("e.g. 3000"));
    await user.type(screen.getByPlaceholderText("e.g. 3000"), "3000");
    expect(open).toBeEnabled();
  });

  it("opens a remote tunnel bound privately on a server-chosen port", async () => {
    const { user, open } = await openForm("Remote (-R)");
    await user.type(screen.getByPlaceholderText("e.g. 3000"), "3000");
    await user.click(open);

    await waitFor(() =>
      expect(opened("open_remote_tunnel")).toEqual({
        hostId: "h",
        remotePort: 0,
        remoteBindHost: "127.0.0.1",
        localHost: "127.0.0.1",
        localPort: 3000,
      }),
    );
  });

  it("sends a custom bind address", async () => {
    const { user, open } = await openForm("Remote (-R)");
    const bind = screen.getAllByPlaceholderText("127.0.0.1")[0];
    await user.clear(bind);
    await user.type(bind, "0.0.0.0");
    await user.type(screen.getByPlaceholderText("auto"), "8080");
    await user.type(screen.getByPlaceholderText("e.g. 3000"), "3000");
    await user.click(open);

    await waitFor(() =>
      expect(opened("open_remote_tunnel")).toMatchObject({
        remoteBindHost: "0.0.0.0",
        remotePort: 8080,
      }),
    );
  });

  it("shows a refused tunnel and keeps the form open", async () => {
    failWith = "The server refused to listen on 127.0.0.1:80";
    const { user, open } = await openForm("Remote (-R)");
    await user.type(screen.getByPlaceholderText("e.g. 3000"), "3000");
    await user.click(open);

    expect(await screen.findByText(/refused to listen/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open" })).toBeInTheDocument();
  });

  it("lists tunnels with their last error and closes one", async () => {
    tunnels = [
      tunnel({
        id: 7,
        direction: "remote",
        remotePort: 8080,
        localPort: 3000,
        activeConnections: 2,
        lastError: "Could not reach 127.0.0.1:3000 on this machine",
      }),
    ];
    const user = userEvent.setup();
    render(<TunnelsPane hostId="h" />);

    expect(await screen.findByText("127.0.0.1:8080")).toBeInTheDocument();
    expect(screen.getByText("2 active")).toBeInTheDocument();
    expect(screen.getByText(/could not reach/i)).toBeInTheDocument();

    await user.click(screen.getByTitle("Close this tunnel"));
    await waitFor(() => expect(opened("close_tunnel")).toEqual({ hostId: "h", tunnelId: 7 }));
    expect(await screen.findByText(/no active tunnels/i)).toBeInTheDocument();
  });

  it("refreshes on this host's events only", async () => {
    render(<TunnelsPane hostId="h" />);
    await screen.findByText(/no active tunnels/i);
    const listed = () => calls.filter((call) => call.cmd === "list_tunnels").length;
    const before = listed();

    tunnels = [tunnel({ id: 3 })];
    await emit("tunnel://state", { hostId: "other", tunnelId: 3, kind: "opened" });
    expect(listed()).toBe(before);

    await emit("tunnel://state", { hostId: "h", tunnelId: 3, kind: "opened" });
    const row = await screen.findByText("127.0.0.1:15432");
    expect(within(row.parentElement!).getByText("local")).toBeInTheDocument();
  });
});
