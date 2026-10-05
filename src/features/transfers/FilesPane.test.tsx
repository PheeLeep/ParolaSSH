import { mockIPC } from "@tauri-apps/api/mocks";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import { createRef } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { RemoteEntry } from "../hosts/types";
import { FilesPane, type FilesPaneHandle } from "./FilesPane";
import { localCrumbs } from "./LocalPane";

/** A fake remote filesystem with the backend's rules: rename and copy never
 *  land on an existing path. Keys are absolute paths; dirs list their children. */
let fs: Map<string, RemoteEntry["kind"]>;
let calls: { cmd: string; args: Record<string, unknown> }[];
let dialogAnswer: string | null;

const HOME = "/home/u";

function entry(path: string): RemoteEntry {
  const name = path.slice(path.lastIndexOf("/") + 1);
  return { name, path, kind: fs.get(path)!, size: 10, modified: null, mode: 0o644, target: null };
}

function children(dir: string): RemoteEntry[] {
  return [...fs.keys()]
    .filter((path) => path.startsWith(`${dir}/`) && !path.slice(dir.length + 1).includes("/"))
    .sort()
    .map(entry);
}

function seed(paths: Record<string, RemoteEntry["kind"]>) {
  fs = new Map(Object.entries(paths));
}

beforeEach(() => {
  calls = [];
  dialogAnswer = null;
  seed({ [HOME]: "dir" });
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, string>;
    calls.push({ cmd, args });
    switch (cmd) {
      case "remote_home_dir":
        return HOME;
      case "list_remote_dir":
        return { path: args.path, entries: children(args.path), truncated: false };
      case "remote_conflicts":
        return (args.names as unknown as string[]).filter((name) =>
          fs.has(`${args.remoteDir}/${name}`),
        );
      case "copy_remote_entry":
      case "rename_remote_entry":
        if (fs.has(args.to)) throw `${args.to} already exists. Rename or remove it first.`;
        fs.set(args.to, fs.get(args.from)!);
        if (cmd === "rename_remote_entry") fs.delete(args.from);
        return args.to;
      case "delete_remote_entry":
        if (args.path.endsWith("locked.txt")) throw "Permission denied";
        fs.delete(args.path);
        return null;
      case "create_remote_dir":
        fs.set(`${args.path}/${args.name}`, "dir");
        return `${args.path}/${args.name}`;
      case "plugin:dialog|open":
        return dialogAnswer;
      case "local_conflicts":
        return [];
      case "enqueue_download":
      case "enqueue_upload":
        return 1;
      case "list_local_tree":
        return {
          files: [{ path: `${args.path}/conf/a.conf`, relative: "conf/a.conf", size: 1 }],
          skipped: [],
          truncated: false,
        };
      case "list_transfers":
        return [];
      case "transfer_summary":
        return { running: 0, queued: 0, maxConcurrent: 3 };
    }
  });
});

const made = (cmd: string) => calls.filter((call) => call.cmd === cmd).map((call) => call.args);

async function renderPane() {
  const user = userEvent.setup();
  render(<FilesPane hostId="h" />);
  await screen.findByRole("navigation", { name: "Current folder" });
  return user;
}

const row = (name: string) => screen.getByText(name).closest("tr")!;

async function openFolder(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(within(row(name)).getByRole("button", { name: "Open" }));
  await waitFor(() =>
    expect(screen.getByRole("navigation", { name: "Current folder" })).toHaveTextContent(name),
  );
}

describe("FilesPane", () => {
  it("opens in the home folder and navigates by breadcrumb", async () => {
    seed({ [HOME]: "dir", [`${HOME}/docs`]: "dir", [`${HOME}/docs/x.txt`]: "file" });
    const user = await renderPane();
    expect(await screen.findByText("docs")).toBeInTheDocument();

    await openFolder(user, "docs");
    expect(await screen.findByText("x.txt")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "u" }));
    expect(await screen.findByText("docs")).toBeInTheDocument();
  });

  it("filters rows and says when nothing matches", async () => {
    seed({ [HOME]: "dir", [`${HOME}/a.txt`]: "file", [`${HOME}/b.log`]: "file" });
    const user = await renderPane();
    await screen.findByText("a.txt");

    await user.type(screen.getByLabelText("Filter files"), "LOG");
    expect(screen.queryByText("a.txt")).not.toBeInTheDocument();
    expect(screen.getByText("b.log")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Filter files"), "zzz");
    expect(screen.getByText("Nothing matches that filter.")).toBeInTheDocument();
  });

  it("lists a symlink but will not let it be selected or acted on", async () => {
    seed({ [HOME]: "dir", [`${HOME}/link`]: "symlink" });
    await renderPane();
    await screen.findByText("link");

    expect(screen.getByLabelText("Select link")).toBeDisabled();
    for (const button of within(row("link")).getAllByRole("button")) {
      expect(button).toBeDisabled();
    }
  });

  it("shows why the home folder could not be read", async () => {
    mockIPC((cmd) => {
      if (cmd === "remote_home_dir") throw "SFTP subsystem refused";
    });
    render(<FilesPane hostId="h" />);
    expect(await screen.findByText("SFTP subsystem refused")).toBeInTheDocument();
  });

  it("deletes a batch only after confirming, and reports a partial failure", async () => {
    seed({
      [HOME]: "dir",
      [`${HOME}/a.txt`]: "file",
      [`${HOME}/locked.txt`]: "file",
    });
    const user = await renderPane();
    await screen.findByText("a.txt");

    await user.click(screen.getByLabelText("Select every listed file"));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(made("delete_remote_entry")).toHaveLength(0);

    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: /delete/i }));

    expect(await screen.findByText("locked.txt: Permission denied")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("a.txt")).not.toBeInTheDocument());
    expect(fs.has(`${HOME}/locked.txt`)).toBe(true);
  });

  it("pastes a copy without asking when nothing clashes", async () => {
    seed({ [HOME]: "dir", [`${HOME}/a.txt`]: "file", [`${HOME}/dst`]: "dir" });
    const user = await renderPane();
    await screen.findByText("a.txt");

    await user.click(screen.getByLabelText("Select a.txt"));
    await user.click(screen.getByRole("button", { name: "Copy" }));
    await openFolder(user, "dst");
    await user.click(screen.getByRole("button", { name: /paste 1/i }));

    await waitFor(() => expect(fs.has(`${HOME}/dst/a.txt`)).toBe(true));
    expect(fs.has(`${HOME}/a.txt`)).toBe(true);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps both under a name that is actually free", async () => {
    seed({
      [HOME]: "dir",
      [`${HOME}/a.txt`]: "file",
      [`${HOME}/dst`]: "dir",
      [`${HOME}/dst/a.txt`]: "file",
      [`${HOME}/dst/a (1).txt`]: "file",
    });
    const user = await renderPane();
    await screen.findByText("a.txt");

    await user.click(screen.getByLabelText("Select a.txt"));
    await user.click(screen.getByRole("button", { name: "Copy" }));
    await openFolder(user, "dst");
    await user.click(screen.getByRole("button", { name: /paste 1/i }));

    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Keep both" }));

    await waitFor(() => expect(fs.has(`${HOME}/dst/a (2).txt`)).toBe(true));
    expect(screen.queryByText(/already exists/)).not.toBeInTheDocument();
  });

  it("does not offer to overwrite on paste, which the server never does", async () => {
    seed({
      [HOME]: "dir",
      [`${HOME}/a.txt`]: "file",
      [`${HOME}/dst`]: "dir",
      [`${HOME}/dst/a.txt`]: "file",
    });
    const user = await renderPane();
    await screen.findByText("a.txt");

    await user.click(screen.getByLabelText("Select a.txt"));
    await user.click(screen.getByRole("button", { name: "Cut" }));
    await openFolder(user, "dst");
    await user.click(screen.getByRole("button", { name: /paste 1/i }));

    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).queryByRole("button", { name: "Overwrite" })).not.toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: "Skip" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(made("rename_remote_entry")).toHaveLength(0);
    expect(fs.has(`${HOME}/a.txt`)).toBe(true);
  });

  it("still offers overwrite for a download, which can replace a local file", async () => {
    seed({ [HOME]: "dir", [`${HOME}/a.txt`]: "file" });
    dialogAnswer = "/tmp/downloads";
    mockIPC((cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (cmd === "remote_home_dir") return HOME;
      if (cmd === "list_remote_dir") return { path: HOME, entries: children(HOME), truncated: false };
      if (cmd === "plugin:dialog|open") return dialogAnswer;
      if (cmd === "local_conflicts") return ["a.txt"];
      if (cmd === "enqueue_download") return 1;
      if (cmd === "list_transfers") return [];
      if (cmd === "transfer_summary") return { running: 0, queued: 0, maxConcurrent: 3 };
    });
    const user = await renderPane();
    await screen.findByText("a.txt");

    await user.click(within(row("a.txt")).getByTitle("Download"));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Overwrite" }));

    await waitFor(() =>
      expect(made("enqueue_download")[0]).toMatchObject({
        remotePath: `${HOME}/a.txt`,
        localDir: "/tmp/downloads",
        onConflict: "overwrite",
      }),
    );
  });

  it("downloads straight into the local pane's folder without asking", async () => {
    seed({ [HOME]: "dir", [`${HOME}/notes.txt`]: "file" });
    const user = userEvent.setup();
    render(<FilesPane hostId="h" localDir="/home/me/Downloads" />);
    await screen.findByText("notes.txt");

    await user.click(within(row("notes.txt")).getByTitle("Download to /home/me/Downloads"));
    await waitFor(() => expect(made("enqueue_download")).toHaveLength(1));
    expect(made("plugin:dialog|open")).toHaveLength(0);
    expect(made("enqueue_download")[0]).toMatchObject({
      localDir: "/home/me/Downloads",
      elevated: false,
    });
  });

  it("routes every call through the root session when elevated", async () => {
    seed({ [HOME]: "dir", [`${HOME}/notes.txt`]: "file" });
    render(<FilesPane hostId="h" elevated />);
    await screen.findByText("notes.txt");
    expect(made("list_remote_dir").every((args) => args.elevated === true)).toBe(true);
  });

  it("uploads a local folder into the open folder, keeping its tree", async () => {
    seed({ [HOME]: "dir" });
    const ref = createRef<FilesPaneHandle>();
    render(<FilesPane hostId="h" ref={ref} />);
    await screen.findByText("This folder is empty.");

    await act(() =>
      ref.current!.uploadFrom([
        { name: "app", path: "/local/app", kind: "dir", size: 0, modified: null, mode: null, target: null },
        { name: "a.txt", path: "/local/a.txt", kind: "file", size: 1, modified: null, mode: null, target: null },
      ]),
    );

    expect(made("enqueue_upload")).toEqual([
      expect.objectContaining({ localPath: "/local/app/conf/a.conf", remoteDir: HOME, relative: "app/conf/a.conf" }),
      expect.objectContaining({ localPath: "/local/a.txt", remoteDir: HOME, relative: "a.txt" }),
    ]);
  });
});

describe("localCrumbs", () => {
  it("puts Windows drives under This PC", () => {
    expect(localCrumbs("C:\\Users\\me").map((crumb) => [crumb.label, crumb.path])).toEqual([
      ["This PC", ""],
      ["C:", "C:\\"],
      ["Users", "C:\\Users"],
      ["me", "C:\\Users\\me"],
    ]);
  });

  it("keeps POSIX paths as the remote side shows them", () => {
    expect(localCrumbs("/home/me").map((crumb) => crumb.path)).toEqual(["/", "/home", "/home/me"]);
  });
});
