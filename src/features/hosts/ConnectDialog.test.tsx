import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { hostRow } from "../../test/fixtures";
import { ConnectDialog } from "./ConnectDialog";
import type { ConnectionInfo, PassphraseNeed } from "./types";

const connect = vi.fn();
vi.mock("./HostsProvider", () => ({ useHosts: () => ({ connect }) }));

let need: PassphraseNeed;
let pending: [string, boolean][];

beforeEach(() => {
  connect.mockReset();
  connect.mockResolvedValue({ hostId: "h1" } as ConnectionInfo);
  need = { kind: "required" };
  pending = [];
  mockIPC(
    (cmd, args) => {
      const a = args as Record<string, unknown>;
      if (cmd === "set_connect_pending") pending.push([a.hostId as string, a.active as boolean]);
      if (cmd === "host_key_passphrase_need") return need;
    },
    { shouldMockEvents: true },
  );
});

function renderDialog(overrides: Parameters<typeof hostRow>[0] = {}) {
  const onClose = vi.fn();
  const onConnected = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <ConnectDialog host={hostRow(overrides)} onClose={onClose} onConnected={onConnected} />,
  );
  return { user, onClose, onConnected, view };
}

const connectButton = () => screen.getByRole("button", { name: /connect/i });

describe("ConnectDialog", () => {
  it("needs a password before connecting, then sends it", async () => {
    const { user, onClose, onConnected } = renderDialog();
    expect(connectButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Password for pheeleep"), "pass123");
    await user.click(connectButton());

    expect(connect).toHaveBeenCalledWith("h1", {
      password: "pass123",
      remember: false,
      trustUnknown: false,
    });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(onConnected).toHaveBeenCalledWith({ hostId: "h1" });
  });

  it("explains that a remembered password stays in memory", async () => {
    const { user } = renderDialog();
    await user.click(screen.getByLabelText("Remember this password until I quit"));
    expect(screen.getByText(/not in your keychain, and not on disk/)).toBeInTheDocument();

    await user.type(screen.getByLabelText("Password for pheeleep"), "pw{Enter}");
    expect(connect).toHaveBeenCalledWith("h1", expect.objectContaining({ remember: true }));
  });

  it("shows an unknown host key and only trusts it on a second, explicit click", async () => {
    connect.mockRejectedValueOnce("HOSTKEY:unknown SHA256:abc123+/=");
    const { user } = renderDialog();

    await user.type(screen.getByLabelText("Password for pheeleep"), "pw");
    await user.click(connectButton());

    expect(await screen.findByText("SHA256:abc123+/=")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Trust and connect" }));
    expect(connect).toHaveBeenLastCalledWith("h1", expect.objectContaining({ trustUnknown: true }));
  });

  it("shows other failures and lets the user retry", async () => {
    connect.mockRejectedValueOnce("Authentication failed.");
    const { user, onClose } = renderDialog();

    await user.type(screen.getByLabelText("Password for pheeleep"), "wrong");
    await user.click(connectButton());

    expect(await screen.findByText("Authentication failed.")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect(connectButton()).toBeEnabled();
  });

  it("connects straight away with the agent", async () => {
    renderDialog({ authMethod: "agent" });
    await waitFor(() =>
      expect(connect).toHaveBeenCalledWith("h1", {
        password: null,
        remember: false,
        trustUnknown: false,
      }),
    );
  });

  it("asks for a passphrase only when the key is locked", async () => {
    const { user } = renderDialog({ authMethod: "publickey", keyPath: "~/.ssh/id_ed25519" });
    const field = await screen.findByLabelText("Key passphrase");
    expect(connect).not.toHaveBeenCalled();

    await user.type(field, "secret{Enter}");
    // A key passphrase is never remembered, whatever the checkbox state.
    expect(connect).toHaveBeenCalledWith("h1", expect.objectContaining({ password: "secret", remember: false }));
  });

  it("connects without a prompt when the key is not encrypted", async () => {
    need = { kind: "notNeeded" };
    renderDialog({ authMethod: "publickey", keyPath: "~/.ssh/id_ed25519" });
    await waitFor(() => expect(connect).toHaveBeenCalled());
    expect(screen.queryByLabelText("Key passphrase")).not.toBeInTheDocument();
  });

  it("refuses a hardware key and points at the agent", async () => {
    need = { kind: "hardware", algorithm: "Ed25519 (FIDO)" };
    renderDialog({ authMethod: "publickey", keyPath: "~/.ssh/id_ed25519_sk" });

    expect(await screen.findByText(/lives on a security token/)).toBeInTheDocument();
    expect(screen.getByText(/ssh-add -K ~\/.ssh\/id_ed25519_sk/)).toBeInTheDocument();
    expect(connectButton()).toBeDisabled();
    expect(screen.queryByLabelText("Key passphrase")).not.toBeInTheDocument();
  });

  it("marks the attempt pending while open and clears it on close", async () => {
    const { view } = renderDialog();
    await waitFor(() => expect(pending).toEqual([["h1", true]]));
    view.unmount();
    await waitFor(() => expect(pending).toEqual([["h1", true], ["h1", false]]));
  });

  it("connects straight away for auth method none", async () => {
    renderDialog({ authMethod: "none" });
    await waitFor(() => expect(connect).toHaveBeenCalledWith("h1", expect.objectContaining({ password: null })));
  });

  describe("status while connecting", () => {
    /** A connect that stays pending, so the status can be inspected. */
    function hang() {
      let finish: (info: ConnectionInfo) => void = () => {};
      connect.mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
      return () => finish({ hostId: "h1" } as ConnectionInfo);
    }

    it("says only Connecting… by default", async () => {
      hang();
      renderDialog({ authMethod: "agent" });
      await waitFor(() => expect(connect).toHaveBeenCalled());
      await act(() =>
        emit("connect://progress", { hostId: "h1", stage: "authenticating", method: "agent" }),
      );
      expect(screen.getByRole("status")).toHaveTextContent(/^Connecting…$/);
    });

    it("updates the one line to this host's current step when detailed", async () => {
      localStorage.setItem("parolassh:connect-details", "on");
      hang();
      renderDialog({ authMethod: "agent" });
      await waitFor(() => expect(connect).toHaveBeenCalled());
      expect(screen.getByRole("status")).toHaveTextContent(/^Connecting…$/);

      await act(() =>
        emit("connect://progress", { hostId: "h1", stage: "dialing", host: "192.168.56.10", port: 22, viaJump: false }),
      );
      expect(screen.getByRole("status")).toHaveTextContent("Connecting to 192.168.56.10:22…");

      await act(async () => {
        await emit("connect://progress", { hostId: "other", stage: "authenticated" });
        await emit("connect://progress", { hostId: "h1", stage: "authenticating", method: "agent" });
      });
      expect(screen.getAllByRole("status")).toHaveLength(1);
      expect(screen.getByRole("status")).toHaveTextContent("Offering the keys held by your SSH agent…");
    });

    it("names the key check before the attempt when detailed", async () => {
      localStorage.setItem("parolassh:connect-details", "on");
      mockIPC(() => new Promise(() => {}), { shouldMockEvents: true });
      renderDialog({ authMethod: "publickey", keyPath: "~/.ssh/id_ed25519" });
      expect(await screen.findByRole("status")).toHaveTextContent(
        "Checking whether ~/.ssh/id_ed25519 is locked…",
      );
    });

    it("keeps the password form's body quiet in plain mode", async () => {
      hang();
      const { user } = renderDialog();
      await user.type(screen.getByLabelText("Password for pheeleep"), "pw{Enter}");
      expect(screen.getByRole("button", { name: "Connecting…" })).toBeDisabled();
      expect(screen.queryByRole("status")).not.toBeInTheDocument();
    });
  });
});
