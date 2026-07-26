import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Form, Spinner, Table } from "react-bootstrap";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowUp,
  CornerDownRight,
  Download,
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
import { formatBytes } from "./format";

/** Browse a host's filesystem over SFTP.
 *
 *  Two things are deliberately absent. There is no elevation button: SFTP runs
 *  as the user who signed in and the subsystem has no sudo, so a denied path
 *  stays denied until you reconnect as someone else — the error says exactly
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

  const download = async (entry: RemoteEntry) => {
    const dir = await openDialog({
      directory: true,
      multiple: false,
      title: `Download ${entry.name} to…`,
    });
    if (typeof dir !== "string") return;

    setBusy(true);
    try {
      await api.enqueueDownload(hostId, entry.path, dir);
      await transfers.refresh();
    } catch (caught) {
      setError(errorMessage(caught));
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
      await api.enqueueUpload(hostId, chosen, path);
      await transfers.refresh();
      // The new file only appears once it has landed, so this is a courtesy
      // refresh; the Transfers page is where progress actually lives.
      void load(path);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const makeDirectory = async () => {
    if (!path) return;
    const name = window.prompt("New folder name");
    if (!name) return;

    setBusy(true);
    try {
      await api.createRemoteDir(hostId, path, name);
      void load(path);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (entry: RemoteEntry) => {
    const what = entry.kind === "dir" ? "folder" : "file";
    if (!window.confirm(`Delete the ${what} "${entry.name}" on the server?`)) return;

    setBusy(true);
    try {
      await api.deleteRemoteEntry(hostId, entry.path, entry.kind === "dir");
      if (path) void load(path);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const needle = filter.trim().toLowerCase();
  const rows = (listing?.entries ?? []).filter((entry) =>
    needle ? entry.name.toLowerCase().includes(needle) : true,
  );

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

        <Button size="sm" variant="outline-secondary" onClick={() => void upload()} disabled={busy || !path}>
          <Upload className="icon-sm" aria-hidden="true" />
          Upload
        </Button>
        <Button size="sm" variant="outline-secondary" onClick={() => void makeDirectory()} disabled={busy || !path}>
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
                <th>Name</th>
                <th className="text-end">Size</th>
                <th>Modified</th>
                <th>Mode</th>
                <th className="text-end">Actions</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((entry) => (
                <FileRow
                  key={entry.path}
                  entry={entry}
                  busy={busy}
                  onOpen={() => open(entry)}
                  onDownload={() => void download(entry)}
                  onDelete={() => void remove(entry)}
                />
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={5} className="text-center text-body-secondary py-4">
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
  symlink: "Symbolic link — ParolaSSH does not follow links.",
  other: "Not a regular file — device files, sockets and pipes are skipped.",
};

function FileRow({
  entry,
  busy,
  onOpen,
  onDownload,
  onDelete,
}: {
  entry: RemoteEntry;
  busy: boolean;
  onOpen: () => void;
  onDownload: () => void;
  onDelete: () => void;
}) {
  const inert = INERT_REASON[entry.kind];
  const isDir = entry.kind === "dir";

  return (
    <tr
      className={inert ? "files-row files-row--inert" : "files-row"}
      role={isDir ? "button" : undefined}
      onDoubleClick={isDir ? onOpen : undefined}
      title={inert}
    >
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
        {isDir ? "—" : formatBytes(entry.size)}
      </td>
      <td className="text-body-secondary small">{formatModified(entry.modified)}</td>
      <td className="font-monospace small text-body-secondary">
        {entry.mode === null ? "—" : entry.mode.toString(8).padStart(4, "0")}
      </td>
      <td className="text-end">
        <div className="d-inline-flex gap-1" onClick={(event) => event.stopPropagation()}>
          {isDir ? (
            <Button size="sm" variant="outline-secondary" onClick={onOpen} disabled={!!inert}>
              Open
            </Button>
          ) : (
            <Button
              size="sm"
              variant="outline-secondary"
              onClick={onDownload}
              disabled={busy || !!inert}
              title={inert ?? "Download"}
            >
              <Download className="icon-sm" aria-hidden="true" />
            </Button>
          )}
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

function formatModified(seconds: number | null): string {
  if (seconds === null) return "—";
  return new Date(seconds * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
