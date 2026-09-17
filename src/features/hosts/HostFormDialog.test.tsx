import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { hostRow } from "../../test/fixtures";
import type { SshKey } from "../keys/types";
import type { HostRow } from "./HostsProvider";
import { HostFormDialog } from "./HostFormDialog";
import { draftFromHost, emptyDraft, type HostDraft, type ProbeResult } from "./types";

const save = vi.fn();
let hosts: HostRow[];
let keys: SshKey[];
vi.mock("./HostsProvider", () => ({ useHosts: () => ({ hosts, save }) }));
vi.mock("../keys/KeysProvider", () => ({ useKeys: () => ({ keys }) }));

let probe: ProbeResult;

beforeEach(() => {
  hosts = [];
  keys = [];
  save.mockReset();
  save.mockImplementation(async (draft: HostDraft) => ({ ...draft, id: draft.id ?? "new-id" }));
  probe = {
    hostname: "192.168.56.10",
    port: 22,
    reachable: true,
    isSsh: true,
    banner: "SSH-2.0-OpenSSH_9.6",
    latencyMs: 3,
    message: "An SSH server answered.",
    authMethods: ["publickey"],
    logs: [],
  };
  mockIPC((cmd) => {
    if (cmd === "list_host_groups") return ["Default", "Lab"];
    if (cmd === "list_host_tags") return ["web"];
    if (cmd === "probe_host") return probe;
  });
});

function renderForm(draft: HostDraft) {
  const onClose = vi.fn();
  const onSaved = vi.fn();
  const user = userEvent.setup();
  render(<HostFormDialog draft={draft} onClose={onClose} onSaved={onSaved} />);
  return { user, onClose, onSaved };
}

describe("HostFormDialog", () => {
  it("adds a connection once hostname and username are filled", async () => {
    const { user, onSaved, onClose } = renderForm(emptyDraft());
    const add = screen.getByRole("button", { name: "Add connection" });
    expect(add).toBeDisabled();

    await user.type(screen.getByLabelText("Hostname or IP"), "10.0.0.5");
    expect(add).toBeDisabled();
    await user.type(screen.getByLabelText("Username"), "admin");
    await user.clear(screen.getByLabelText("Port"));
    await user.type(screen.getByLabelText("Port"), "2222");
    await user.type(screen.getByLabelText("Tags"), "db,prod{Enter}");
    await user.click(add);

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        hostname: "10.0.0.5",
        username: "admin",
        port: 2222,
        tags: ["db", "prod"],
        authMethod: "password",
      }),
    );
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith("new-id"));
    expect(onClose).toHaveBeenCalled();
  });

  it("keeps the dialog open with the reason when saving fails", async () => {
    save.mockRejectedValueOnce("That connection no longer exists - it may have been deleted.");
    const { user, onClose } = renderForm(draftFromHost(hostRow()));

    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByText(/no longer exists/)).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("warns when public key auth has no keys to offer", async () => {
    const { user } = renderForm(emptyDraft());
    await user.selectOptions(screen.getByLabelText("Authentication"), "publickey");
    expect(screen.getByText(/No keys found/)).toBeInTheDocument();
  });

  it("warns when the chosen method was not advertised by the server", async () => {
    const { user } = renderForm({ ...emptyDraft(), hostname: "192.168.56.10" });
    await user.click(screen.getByRole("button", { name: "Check port" }));

    expect(await screen.findByText("An SSH server answered.")).toBeInTheDocument();
    expect(screen.getByText(/was not advertised by the server/)).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Authentication"), "agent");
    expect(screen.queryByText(/was not advertised by the server/)).not.toBeInTheDocument();
  });

  it("never offers a jump host that would loop back", async () => {
    hosts = [
      hostRow({ id: "self", label: "target" }),
      hostRow({ id: "via-self", label: "behind-target", proxyJump: "self" }),
      hostRow({ id: "deeper", label: "behind-that", proxyJump: "via-self" }),
      hostRow({ id: "bastion", label: "bastion" }),
    ];
    renderForm(draftFromHost(hosts[0]));

    const options = within(screen.getByLabelText("Jump host"))
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(options).toEqual(["Connect directly", expect.stringContaining("bastion")]);
  });

  it("explains what auth method none really means", async () => {
    const { user } = renderForm(emptyDraft());
    await user.selectOptions(screen.getByLabelText("Authentication"), "none");
    expect(screen.getByText(/an ordinary sshd, and any Windows host/)).toBeInTheDocument();
  });
});

describe("TagInput via the form", () => {
  it("ignores a duplicate in another case and removes chips", async () => {
    const { user } = renderForm({ ...emptyDraft(), tags: ["Web"] });
    const field = screen.getByLabelText("Tags");

    await user.type(field, "web{Enter}");
    expect(screen.getAllByText(/^web$/i)).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Remove tag Web" }));
    expect(screen.queryByText("Web")).not.toBeInTheDocument();
  });

  it("commits a typed tag on blur so Save does not lose it", async () => {
    const { user } = renderForm(draftFromHost(hostRow()));
    await user.type(screen.getByLabelText("Tags"), "staging");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ tags: ["staging"] }));
  });
});
