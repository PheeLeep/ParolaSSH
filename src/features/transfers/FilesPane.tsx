import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Form, Spinner, Table } from "react-bootstrap";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowUp,
  ClipboardPaste,
  Copy,
  CornerDownRight,
  Download,
  PencilLine,
  Scissors,
  File as FileIcon,
  Folder,
  FolderPlus,
  Link2,
  RefreshCw,
  Trash2,
  Upload,
} from "lucide-react";

import * as api from "../hosts/api";
import { errorMessage } from "../hosts/api";
import type { DirListing, RemoteEntry } from "../hosts/types";
import * as transfers from "./transferStore";
import {
  ConflictDialog,
  DeleteEntriesDialog,
  NewFolderDialog,
  RenameDialog,
  type ConflictChoice,
  type ConflictPrompt,
} from "./FilesDialogs";
import { formatBytes } from "./format";
import { readDefaultTransferPriority } from "../settings/preferences";
import * as toast from "../../lib/toast";

/** Browse a host's filesystem over SFTP.
 *
 *  Two things are deliberately absent. There is no elevation button: SFTP runs
 *  as the user who signed in and the subsystem has no sudo, so a denied path
 *  stays denied until you reconnect as someone else - the error says exactly
 *  that rather than offering a prompt that cannot work.
 *
 *  And symlinks are shown but never opened. A link is the cheapest way for a
 *  host to point a download somewhere it should not go, so rows for links are
 *  inert and say why.
 */
export function FilesPane({ hostId }: { hostId: string }) {
  const [listing, setListing] = useState<DirListing | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  /** Batch selection, keyed by remote path so it survives a refresh in place. */
  const [selected, setSelected] = useState<Set<string>>(new Set());

  /** Both dialogs keep their own error, shown next to the action that failed
   *  rather than in the pane behind them. */
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderError, setNewFolderError] = useState<string | null>(null);
  /** What a confirmed delete would remove. Empty means the dialog is closed. */
  const [pendingDelete, setPendingDelete] = useState<RemoteEntry[]>([]);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<RemoteEntry | null>(null);
  const [renameError, setRenameError] = useState<string | null>(null);

  /** Cut or copied entries, waiting for a Paste. Held in the pane rather than a
   *  module store: a clipboard that survived navigating to another host would
   *  paste paths that mean nothing there. */
  const [clipboard, setClipboard] = useState<{
    mode: "copy" | "cut";
    entries: RemoteEntry[];
  } | null>(null);

  /** Promise-based conflict prompt, in the shape `ElevationProvider` uses: the
   *  async action awaits an answer the user gives through the dialog. */
  const [conflict, setConflict] = useState<ConflictPrompt | null>(null);
  const settleConflict = useRef<
    ((value: { choice: ConflictChoice; applyToAll: boolean } | null) => void) | null
  >(null);

  const askConflict = (prompt: ConflictPrompt) =>
    new Promise<{ choice: ConflictChoice; applyToAll: boolean } | null>((resolve) => {
      settleConflict.current = resolve;
      setConflict(prompt);
    });

  const resolveConflict = (
    value: { choice: ConflictChoice; applyToAll: boolean } | null,
  ) => {
    settleConflict.current?.(value);
    settleConflict.current = null;
    setConflict(null);
  };

  /** Ask about each clash in turn and return the plan to actually run.
   *  `null` means the user closed the dialog, which cancels the whole batch. */
  const planAgainstConflicts = async <T,>(
    items: T[],
    nameOf: (item: T) => string,
    taken: Set<string>,
    destination: string,
  ): Promise<{ item: T; onConflict: "overwrite" | "keepBoth" }[] | null> => {
    const clashes = items.filter((item) => taken.has(nameOf(item)));
    let blanket: ConflictChoice | null = null;
    const decisions = new Map<string, ConflictChoice>();

    for (const [index, item] of clashes.entries()) {
      if (blanket) break;
      const answer = await askConflict({
        name: nameOf(item),
        destination,
        remaining: clashes.length - index,
      });
      if (!answer) return null;
      if (answer.applyToAll) blanket = answer.choice;
      else decisions.set(nameOf(item), answer.choice);
    }

    const plan: { item: T; onConflict: "overwrite" | "keepBoth" }[] = [];
    for (const item of items) {
      const name = nameOf(item);
      if (!taken.has(name)) {
        plan.push({ item, onConflict: "keepBoth" });
        continue;
      }
      const choice = blanket ?? decisions.get(name) ?? "skip";
      if (choice === "skip") continue;
      plan.push({ item, onConflict: choice });
    }
    return plan;
  };

  /** Directory clicks can outrun their responses; only the newest one may
   *  paint. Without this, double-clicking into a deep tree can land you back in
   *  a parent when the slower request resolves last. */
  const requestId = useRef(0);

  const load = useCallback(
    async (target: string) => {
      const id = ++requestId.current;
      setLoading(true);
      setError(null);
      try {
        const next = await api.listRemoteDir(hostId, target);
        if (id !== requestId.current) return;
        setListing(next);
        setPath(next.path);
        // Rows that are gone cannot stay selected - including after a delete.
        setSelected((current) => {
          if (current.size === 0) return current;
          const paths = new Set(next.entries.map((entry) => entry.path));
          return new Set([...current].filter((entry) => paths.has(entry)));
        });
      } catch (caught) {
        if (id !== requestId.current) return;
        setError(errorMessage(caught));
      } finally {
        if (id === requestId.current) setLoading(false);
      }
    },
    [hostId],
  );

  // Start in the user's own directory, which the subsystem tells us.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const home = await api.remoteHomeDir(hostId);
        if (!cancelled) void load(home);
      } catch (caught) {
        if (!cancelled) {
          setError(errorMessage(caught));
          setLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [hostId, load]);

  const open = (entry: RemoteEntry) => {
    if (entry.kind !== "dir") return;
    void load(entry.path);
  };

  const goUp = () => {
    if (!path || path === "/") return;
    void load(path.slice(0, path.lastIndexOf("/")) || "/");
  };

  /** Download files and/or folders. Folders are walked first, so what is queued
   *  is a flat list of regular files with their tree position preserved. */
  const downloadEntries = async (entries: RemoteEntry[]) => {
    const usable = entries.filter((entry) => !INERT_REASON[entry.kind]);
    if (usable.length === 0) return;

    const dir = await openDialog({
      directory: true,
      multiple: false,
      title:
        usable.length === 1
          ? `Download ${usable[0].name} to…`
          : `Download ${usable.length} items to…`,
    });
    if (typeof dir !== "string") return;

    setBusy(true);
    setError(null);
    const walking = toast.progress("Preparing download…");

    try {
      // { remotePath, relative } - `relative` is where it lands under `dir`.
      const planned: { remotePath: string; relative: string }[] = [];
      const skipped: string[] = [];
      let truncated = false;

      for (const entry of usable) {
        if (entry.kind !== "dir") {
          planned.push({ remotePath: entry.path, relative: entry.name });
          continue;
        }
        const tree = await api.listRemoteTree(hostId, entry.path);
        skipped.push(...tree.skipped);
        truncated = truncated || tree.truncated;
        for (const file of tree.files) {
          planned.push({
            remotePath: file.path,
            // Keep the folder itself as the top level, so downloading `app/`
            // gives you `app/…` rather than loose files.
            relative: `${entry.name}/${file.relative}`,
          });
        }
      }

      if (planned.length === 0) {
        walking.fail("Nothing to download", "That folder holds no regular files.");
        return;
      }

      const taken = new Set(
        await api.localConflicts(dir, planned.map((item) => item.relative)),
      );
      walking.dismiss();

      const plan = await planAgainstConflicts(
        planned,
        (item) => item.relative,
        taken,
        dir,
      );
      if (!plan) return; // Cancelled at the conflict dialog.
      if (plan.length === 0) {
        toast.success("Nothing queued", "Every file was skipped.");
        return;
      }

      const priority = readDefaultTransferPriority();
      for (const { item, onConflict } of plan) {
        await api.enqueueDownload(hostId, item.remotePath, dir, {
          relative: item.relative,
          onConflict,
          priority,
        });
      }
      await transfers.refresh();
      setSelected(new Set());

      const notes: string[] = [];
      if (skipped.length > 0) {
        notes.push(`${skipped.length} link${skipped.length === 1 ? "" : "s"} or special file${skipped.length === 1 ? "" : "s"} skipped.`);
      }
      if (truncated) notes.push("The folder was too large to list in full.");
      toast.success(
        `Queued ${plan.length} file${plan.length === 1 ? "" : "s"}`,
        notes.join(" ") || undefined,
      );
    } catch (caught) {
      const message = errorMessage(caught);
      setError(message);
      walking.fail("Could not queue that download", message);
    } finally {
      setBusy(false);
    }
  };

  const upload = async () => {
    if (!path) return;
    const chosen = await openDialog({ multiple: false, title: "Upload a file" });
    if (typeof chosen !== "string") return;

    setBusy(true);
    try {
      await api.enqueueUpload(hostId, chosen, path, {
        priority: readDefaultTransferPriority(),
      });
      await transfers.refresh();
      toast.success("Upload queued", chosen.split(/[/\\]/).pop());
      // The new file only appears once it has landed, so this is a courtesy
      // refresh; the Transfers page is where progress actually lives.
      void load(path);
    } catch (caught) {
      const message = errorMessage(caught);
      setError(message);
      toast.error("Could not upload that file", message);
    } finally {
      setBusy(false);
    }
  };

  const makeDirectory = async (name: string) => {
    if (!path) return;

    setBusy(true);
    setNewFolderError(null);
    try {
      await api.createRemoteDir(hostId, path, name);
      setNewFolderOpen(false);
      toast.success("Folder created", name);
      void load(path);
    } catch (caught) {
      // Stays in the dialog: the name is still in the field and usually only
      // needs an edit, so closing would throw the typing away.
      setNewFolderError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  /** Both the row button and the batch bar route through the same dialog, so
   *  there is no path to a delete that was never confirmed. */
  const askDelete = (targets: RemoteEntry[]) => {
    setDeleteError(null);
    setPendingDelete(targets);
  };

  const confirmDelete = async () => {
    const targets = pendingDelete;
    if (targets.length === 0) return;

    const label =
      targets.length === 1 ? targets[0].name : `${targets.length} items`;
    const running = toast.progress(`Deleting ${label}…`);

    setBusy(true);
    setDeleteError(null);

    // One failure should not hide the rest: keep going, report what broke.
    const failed: string[] = [];
    for (const entry of targets) {
      try {
        await api.deleteRemoteEntry(hostId, entry.path, entry.kind === "dir");
      } catch (caught) {
        failed.push(`${entry.name}: ${errorMessage(caught)}`);
      }
    }
    setBusy(false);

    if (failed.length === targets.length) {
      // Nothing went: keep the dialog open with the reason.
      running.fail(`Could not delete ${label}`, failed.join("\n"));
      setDeleteError(failed.join("\n"));
      if (path) void load(path);
      return;
    }

    const deleted = targets.length - failed.length;
    if (failed.length > 0) {
      running.fail(
        `Deleted ${deleted} of ${targets.length}`,
        failed.join("\n"),
      );
      setError(failed.join("\n"));
    } else {
      running.succeed(`Deleted ${label}`);
    }

    setPendingDelete([]);
    setSelected(new Set());
    if (path) void load(path);
  };

  const needle = filter.trim().toLowerCase();
  const rows = (listing?.entries ?? []).filter((entry) =>
    needle ? entry.name.toLowerCase().includes(needle) : true,
  );

  const selectable = rows.filter((entry) => !INERT_REASON[entry.kind]);
  const selectedRows = selectable.filter((entry) => selected.has(entry.path));
  const allSelected = selectable.length > 0 && selectedRows.length === selectable.length;

  const toggleOne = (entry: RemoteEntry) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(entry.path)) next.add(entry.path);
      return next;
    });

  /** The header box works on what is on screen, so a filter narrows its reach. */
  const toggleAll = () =>
    setSelected((current) => {
      const next = new Set(current);
      for (const entry of selectable) {
        if (allSelected) next.delete(entry.path);
        else next.add(entry.path);
      }
      return next;
    });

  /** Rename in place, and the clipboard operations that share its plumbing. */
  const rename = async (name: string) => {
    if (!renaming) return;
    const target = `${parentPath(renaming.path)}/${name}`.replace("//", "/");

    setBusy(true);
    setRenameError(null);
    try {
      await api.renameRemoteEntry(hostId, renaming.path, target);
      setRenaming(null);
      toast.success("Renamed", `${renaming.name} → ${name}`);
      if (path) void load(path);
    } catch (caught) {
      // Stays open: the typed name is usually one edit from working.
      setRenameError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  /** Paste whatever is on the clipboard into the folder on screen.
   *
   *  A copy is done by the server (`cp`), a cut by SFTP rename - neither moves
   *  bytes across the network, so both are effectively instant even for large
   *  trees. Conflicts are resolved first, because rename refuses to overwrite
   *  and `cp` would happily clobber. */
  const paste = async () => {
    if (!clipboard || !path) return;
    const { mode, entries } = clipboard;

    setBusy(true);
    setError(null);
    const running = toast.progress(
      `${mode === "copy" ? "Copying" : "Moving"} ${entries.length} item${entries.length === 1 ? "" : "s"}…`,
    );

    try {
      const taken = new Set(
        await api.remoteConflicts(hostId, path, entries.map((entry) => entry.name)),
      );
      const plan = await planAgainstConflicts(
        entries,
        (entry) => entry.name,
        taken,
        path,
      );
      if (!plan) {
        running.dismiss();
        return;
      }

      const failed: string[] = [];
      for (const { item, onConflict } of plan) {
        // "Keep both" needs a free name; the server's own operations do not
        // disambiguate, so it is resolved here before asking for either.
        const name =
          onConflict === "keepBoth" && taken.has(item.name)
            ? freeName(item.name, taken)
            : item.name;
        const target = `${path}/${name}`.replace("//", "/");
        try {
          if (mode === "copy") await api.copyRemoteEntry(hostId, item.path, target);
          else await api.renameRemoteEntry(hostId, item.path, target);
          taken.add(name);
        } catch (caught) {
          failed.push(`${item.name}: ${errorMessage(caught)}`);
        }
      }

      const done = plan.length - failed.length;
      if (failed.length > 0) {
        running.fail(
          `${done} of ${plan.length} ${mode === "copy" ? "copied" : "moved"}`,
          failed.join("\n"),
        );
        setError(failed.join("\n"));
      } else {
        running.succeed(
          `${mode === "copy" ? "Copied" : "Moved"} ${done} item${done === 1 ? "" : "s"}`,
        );
      }

      // A cut is spent once pasted; a copy can be pasted again elsewhere.
      if (mode === "cut") setClipboard(null);
      setSelected(new Set());
      void load(path);
    } catch (caught) {
      const message = errorMessage(caught);
      setError(message);
      running.fail("Could not paste", message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="files-pane">
      <div className="files-pane__bar">
        <Button
          size="sm"
          variant="outline-secondary"
          onClick={goUp}
          disabled={!path || path === "/" || loading}
          title="Up one folder"
        >
          <ArrowUp className="icon-sm" aria-hidden="true" />
        </Button>

        <Breadcrumbs path={path} onNavigate={(next) => void load(next)} />

        <Form.Control
          size="sm"
          className="files-pane__filter"
          placeholder="Filter"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          aria-label="Filter files"
        />

        {clipboard && (
          <Button
            size="sm"
            variant="outline-primary"
            onClick={() => void paste()}
            disabled={busy || !path}
            title={`${clipboard.mode === "copy" ? "Copy" : "Move"} ${clipboard.entries.length} item(s) here`}
          >
            <ClipboardPaste className="icon-sm" aria-hidden="true" />
            Paste {clipboard.entries.length}
          </Button>
        )}
        <Button size="sm" variant="outline-secondary" onClick={() => void upload()} disabled={busy || !path}>
          <Upload className="icon-sm" aria-hidden="true" />
          Upload
        </Button>
        <Button size="sm" variant="outline-secondary" onClick={() => { setNewFolderError(null); setNewFolderOpen(true); }} disabled={busy || !path}>
          <FolderPlus className="icon-sm" aria-hidden="true" />
          New folder
        </Button>
        <Button
          size="sm"
          variant="outline-secondary"
          onClick={() => path && void load(path)}
          disabled={loading || !path}
          title="Refresh"
        >
          <RefreshCw className="icon-sm" aria-hidden="true" />
        </Button>
      </div>

      {error && (
        <Alert variant="danger" className="text-prewrap mb-0">
          {error}
        </Alert>
      )}

      {listing?.truncated && (
        <Alert variant="warning" className="mb-0 py-2 small">
          This folder holds more entries than can be listed at once. Use the
          filter, or open a subfolder.
        </Alert>
      )}

      {selectedRows.length > 0 && (
        <div className="files-pane__batch">
          <span className="fw-medium">
            {selectedRows.length} selected
          </span>
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={() => void downloadEntries(selectedRows)}
            disabled={busy}
          >
            <Download className="icon-sm" aria-hidden="true" />
            Download
          </Button>
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={() => setClipboard({ mode: "copy", entries: selectedRows })}
            disabled={busy}
          >
            <Copy className="icon-sm" aria-hidden="true" />
            Copy
          </Button>
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={() => setClipboard({ mode: "cut", entries: selectedRows })}
            disabled={busy}
          >
            <Scissors className="icon-sm" aria-hidden="true" />
            Cut
          </Button>
          <Button
            size="sm"
            variant="outline-danger"
            onClick={() => askDelete(selectedRows)}
            disabled={busy}
          >
            <Trash2 className="icon-sm" aria-hidden="true" />
            Delete
          </Button>
          <Button
            size="sm"
            variant="link"
            className="ms-auto"
            onClick={() => setSelected(new Set())}
          >
            Clear
          </Button>
        </div>
      )}

      <NewFolderDialog
        show={newFolderOpen}
        parent={path}
        busy={busy}
        error={newFolderError}
        onCancel={() => setNewFolderOpen(false)}
        onCreate={(name) => void makeDirectory(name)}
      />

      <RenameDialog
        entry={renaming}
        busy={busy}
        error={renameError}
        onCancel={() => setRenaming(null)}
        onRename={(name) => void rename(name)}
      />

      <ConflictDialog
        prompt={conflict}
        onResolve={(choice, applyToAll) => resolveConflict({ choice, applyToAll })}
        onCancel={() => resolveConflict(null)}
      />

      <DeleteEntriesDialog
        targets={pendingDelete}
        busy={busy}
        error={deleteError}
        onCancel={() => setPendingDelete([])}
        onConfirm={() => void confirmDelete()}
      />

      <div className="files-table">
        {loading && !listing ? (
          <div className="d-flex align-items-center gap-2 p-3 text-body-secondary">
            <Spinner animation="border" size="sm" />
            Reading the folder…
          </div>
        ) : (
          <Table size="sm" className="align-middle mb-0">
            <thead>
              <tr>
                <th className="files-table__check">
                  <RowCheck
                    checked={allSelected}
                    indeterminate={selectedRows.length > 0}
                    disabled={selectable.length === 0}
                    onChange={toggleAll}
                    label="Select every listed file"
                  />
                </th>
                <th>Name</th>
                <th className="text-end">Size</th>
                <th>Modified</th>
                <th>Mode</th>
                <th className="text-center datatable__sticky datatable__sticky--right">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((entry) => (
                <FileRow
                  key={entry.path}
                  entry={entry}
                  busy={busy}
                  selected={selected.has(entry.path)}
                  onToggle={() => toggleOne(entry)}
                  onOpen={() => open(entry)}
                  onDownload={() => void downloadEntries([entry])}
                  onRename={() => { setRenameError(null); setRenaming(entry); }}
                  onDelete={() => askDelete([entry])}
                />
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={6} className="text-center text-body-secondary py-4">
                    {needle ? "Nothing matches that filter." : "This folder is empty."}
                  </td>
                </tr>
              )}
            </tbody>
          </Table>
        )}
      </div>
    </div>
  );
}

/** A link or a device file is listed but cannot be acted on. The reason rides
 *  on the row rather than appearing as an error after a click, so the refusal
 *  is visible before anyone tries. */
const INERT_REASON: Record<string, string> = {
  symlink: "Symbolic link - ParolaSSH does not follow links.",
  other: "Not a regular file - device files, sockets and pipes are skipped.",
};

function FileRow({
  entry,
  busy,
  selected,
  onToggle,
  onOpen,
  onDownload,
  onRename,
  onDelete,
}: {
  entry: RemoteEntry;
  busy: boolean;
  selected: boolean;
  onToggle: () => void;
  onOpen: () => void;
  onDownload: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  const inert = INERT_REASON[entry.kind];
  const isDir = entry.kind === "dir";

  return (
    <tr
      className={`files-row${inert ? " files-row--inert" : ""}${selected ? " table-active" : ""}`}
      role={isDir ? "button" : undefined}
      onDoubleClick={isDir ? onOpen : undefined}
      title={inert}
    >
      <td className="files-table__check">
        <RowCheck
          checked={selected}
          disabled={!!inert}
          onChange={onToggle}
          label={`Select ${entry.name}`}
        />
      </td>
      <td>
        <span className="d-inline-flex align-items-center gap-2">
          <EntryIcon kind={entry.kind} />
          <span className={isDir ? "fw-medium" : undefined}>{entry.name}</span>
          {entry.target && (
            <span className="text-body-secondary small d-inline-flex align-items-center gap-1">
              <CornerDownRight className="icon-sm" aria-hidden="true" />
              {entry.target}
            </span>
          )}
        </span>
      </td>
      <td className="text-end font-monospace small">
        {isDir ? "-" : formatBytes(entry.size)}
      </td>
      <td className="text-body-secondary small">{formatModified(entry.modified)}</td>
      <td className="font-monospace small text-body-secondary">
        {entry.mode === null ? "-" : entry.mode.toString(8).padStart(4, "0")}
      </td>
      <td className="text-center datatable__sticky datatable__sticky--right">
        <div className="d-inline-flex gap-1" onClick={(event) => event.stopPropagation()}>
          {isDir && (
            <Button size="sm" variant="outline-secondary" onClick={onOpen} disabled={!!inert}>
              Open
            </Button>
          )}
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={onDownload}
            disabled={busy || !!inert}
            title={inert ?? (isDir ? "Download this folder" : "Download")}
          >
            <Download className="icon-sm" aria-hidden="true" />
          </Button>
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={onRename}
            disabled={busy || !!inert}
            title={inert ?? "Rename"}
          >
            <PencilLine className="icon-sm" aria-hidden="true" />
          </Button>
          <Button
            size="sm"
            variant="outline-danger"
            onClick={onDelete}
            disabled={busy || !!inert}
            title={inert ?? "Delete on the server"}
          >
            <Trash2 className="icon-sm" aria-hidden="true" />
          </Button>
        </div>
      </td>
    </tr>
  );
}

function RowCheck({
  checked,
  indeterminate = false,
  disabled = false,
  onChange,
  label,
}: {
  checked: boolean;
  indeterminate?: boolean;
  disabled?: boolean;
  onChange: () => void;
  label: string;
}) {
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !checked && indeterminate;
  }, [checked, indeterminate]);

  return (
    <input
      ref={ref}
      type="checkbox"
      className="form-check-input m-0"
      checked={checked}
      disabled={disabled}
      onChange={onChange}
      aria-label={label}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
    />
  );
}

function EntryIcon({ kind }: { kind: RemoteEntry["kind"] }) {
  if (kind === "dir") return <Folder className="icon-sm text-primary" aria-hidden="true" />;
  if (kind === "symlink") return <Link2 className="icon-sm text-body-secondary" aria-hidden="true" />;
  return <FileIcon className="icon-sm text-body-secondary" aria-hidden="true" />;
}

function Breadcrumbs({
  path,
  onNavigate,
}: {
  path: string | null;
  onNavigate: (path: string) => void;
}) {
  if (!path) return <span className="files-pane__path text-body-secondary">…</span>;

  const parts = path.split("/").filter(Boolean);

  return (
    <nav className="files-pane__path" aria-label="Current folder">
      <button type="button" className="files-crumb" onClick={() => onNavigate("/")}>
        /
      </button>
      {parts.map((part, index) => (
        <span key={`${part}-${index}`} className="d-inline-flex align-items-center">
          <button
            type="button"
            className="files-crumb"
            onClick={() => onNavigate(`/${parts.slice(0, index + 1).join("/")}`)}
          >
            {part}
          </button>
          {index < parts.length - 1 && <span className="files-crumb__sep">/</span>}
        </span>
      ))}
    </nav>
  );
}

/** Everything above the last `/`, or the root. */
function parentPath(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut <= 0 ? "" : path.slice(0, cut);
}

/** The backend's "keep both" scheme, applied client-side because the server's
 *  own copy and rename do not disambiguate. */
function freeName(name: string, taken: Set<string>): string {
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const extension = dot > 0 ? name.slice(dot) : "";
  for (let index = 1; index < 1000; index += 1) {
    const candidate = `${stem} (${index})${extension}`;
    if (!taken.has(candidate)) return candidate;
  }
  return `${stem} (${Date.now()})${extension}`;
}

function formatModified(seconds: number | null): string {
  if (seconds === null) return "-";
  return new Date(seconds * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
