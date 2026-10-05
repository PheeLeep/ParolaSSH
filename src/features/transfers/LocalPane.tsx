import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Form, Spinner, Table } from "react-bootstrap";
import { ArrowUp, House, RefreshCw, Upload } from "lucide-react";

import * as api from "../hosts/api";
import { errorMessage } from "../hosts/api";
import type { LocalListing, RemoteEntry } from "../hosts/types";
import {
  Breadcrumbs,
  EntryIcon,
  RowCheck,
  formatModified,
  posixCrumbs,
  type Crumb,
} from "./FilesPane";
import { formatBytes } from "./format";

/** This computer's side of the Files tab: browse, select, upload. Files are
 *  only read here - changes to the local disk belong in the OS file manager. */
export function LocalPane({
  canUpload,
  uploadHint,
  onUpload,
  reloadKey = 0,
  onPathChange,
}: {
  /** False until the remote side has a folder to upload into. */
  canUpload: boolean;
  uploadHint: string;
  onUpload: (entries: RemoteEntry[]) => Promise<void>;
  /** Bumped to reload the folder on screen, e.g. once a download lands. */
  reloadKey?: number;
  onPathChange?: (path: string | null) => void;
}) {
  const [listing, setListing] = useState<LocalListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const requestId = useRef(0);

  const load = useCallback(async (target: string) => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);
    try {
      const next = await api.listLocalDir(target);
      if (id !== requestId.current) return;
      setListing(next);
      setSelected((current) => {
        if (current.size === 0) return current;
        const paths = new Set(next.entries.map((entry) => entry.path));
        return new Set([...current].filter((entry) => paths.has(entry)));
      });
    } catch (caught) {
      if (id === requestId.current) setError(errorMessage(caught));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  const goHome = useCallback(async () => {
    try {
      await load(await api.localHomeDir());
    } catch (caught) {
      setError(errorMessage(caught));
      setLoading(false);
    }
  }, [load]);

  useEffect(() => {
    void goHome();
  }, [goHome]);

  const path = listing?.path ?? null;
  const pathRef = useRef(path);
  pathRef.current = path;

  const firstRender = useRef(true);
  useEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      return;
    }
    if (pathRef.current !== null) void load(pathRef.current);
  }, [reloadKey, load]);

  useEffect(() => {
    // The Windows drive list ("") is not somewhere a file can land.
    onPathChange?.(path || null);
  }, [path, onPathChange]);

  const upload = async (entries: RemoteEntry[]) => {
    setBusy(true);
    try {
      await onUpload(entries);
      setSelected(new Set());
    } finally {
      setBusy(false);
    }
  };

  const needle = filter.trim().toLowerCase();
  const rows = (listing?.entries ?? []).filter((entry) =>
    needle ? entry.name.toLowerCase().includes(needle) : true,
  );
  const selectable = rows.filter((entry) => isUploadable(entry));
  const selectedRows = selectable.filter((entry) => selected.has(entry.path));
  const allSelected = selectable.length > 0 && selectedRows.length === selectable.length;

  const toggleOne = (entry: RemoteEntry) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(entry.path)) next.add(entry.path);
      return next;
    });

  const toggleAll = () =>
    setSelected((current) => {
      const next = new Set(current);
      for (const entry of selectable) {
        if (allSelected) next.delete(entry.path);
        else next.add(entry.path);
      }
      return next;
    });

  return (
    <div className="files-pane">
      <div className="files-pane__bar">
        <Button
          size="sm"
          variant="outline-secondary"
          onClick={() => listing?.parent != null && void load(listing.parent)}
          disabled={listing?.parent == null || loading}
          title="Up one folder"
        >
          <ArrowUp className="icon-sm" aria-hidden="true" />
        </Button>
        <Button size="sm" variant="outline-secondary" onClick={() => void goHome()} title="Home folder">
          <House className="icon-sm" aria-hidden="true" />
        </Button>

        <Breadcrumbs crumbs={listing ? localCrumbs(listing.path) : null} onNavigate={(next) => void load(next)} />

        <Form.Control
          size="sm"
          className="files-pane__filter"
          placeholder="Filter"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          aria-label="Filter local files"
        />
        <Button
          size="sm"
          variant="outline-secondary"
          onClick={() => path !== null && void load(path)}
          disabled={loading || path === null}
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
          <span className="fw-medium">{selectedRows.length} selected</span>
          <Button
            size="sm"
            variant="outline-primary"
            onClick={() => void upload(selectedRows)}
            disabled={busy || !canUpload}
            title={uploadHint}
          >
            <Upload className="icon-sm" aria-hidden="true" />
            Upload
          </Button>
          <Button size="sm" variant="link" className="ms-auto" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
        </div>
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
                <th className="files-table__check">
                  <RowCheck
                    checked={allSelected}
                    indeterminate={selectedRows.length > 0}
                    disabled={selectable.length === 0}
                    onChange={toggleAll}
                    label="Select every listed local file"
                  />
                </th>
                <th>Name</th>
                <th className="text-end">Size</th>
                <th className="files-table__modified">Modified</th>
                <th className="text-center datatable__sticky datatable__sticky--right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((entry) => {
                const isDir = entry.kind === "dir";
                const uploadable = isUploadable(entry);
                return (
                  <tr
                    key={entry.path}
                    className={`files-row${uploadable ? "" : " files-row--inert"}${selected.has(entry.path) ? " table-active" : ""}`}
                    role={isDir ? "button" : undefined}
                    onDoubleClick={isDir ? () => void load(entry.path) : undefined}
                    title={uploadable ? undefined : "Not a regular file or folder"}
                  >
                    <td className="files-table__check">
                      <RowCheck
                        checked={selected.has(entry.path)}
                        disabled={!uploadable}
                        onChange={() => toggleOne(entry)}
                        label={`Select ${entry.name}`}
                      />
                    </td>
                    <td>
                      <span className="d-inline-flex align-items-center gap-2">
                        <EntryIcon kind={entry.kind} />
                        <span className={isDir ? "fw-medium" : undefined}>{entry.name}</span>
                      </span>
                    </td>
                    <td className="text-end font-monospace small">
                      {isDir ? "-" : formatBytes(entry.size)}
                    </td>
                    <td className="text-body-secondary small files-table__modified">
                      {formatModified(entry.modified)}
                    </td>
                    <td className="text-center datatable__sticky datatable__sticky--right">
                      <div className="d-inline-flex gap-1" onClick={(event) => event.stopPropagation()}>
                        {isDir && (
                          <Button size="sm" variant="outline-secondary" onClick={() => void load(entry.path)}>
                            Open
                          </Button>
                        )}
                        {/* No upload from the drive list: a whole drive is not a folder upload. */}
                        {listing?.path !== "" && (
                          <Button
                            size="sm"
                            variant="outline-secondary"
                            onClick={() => void upload([entry])}
                            disabled={busy || !canUpload || !uploadable}
                            title={uploadHint}
                            aria-label={`Upload ${entry.name}`}
                          >
                            <Upload className="icon-sm" aria-hidden="true" />
                          </Button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
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

function isUploadable(entry: RemoteEntry): boolean {
  return entry.kind === "file" || entry.kind === "dir";
}

/** Crumbs for a native path: POSIX as-is, Windows under a "This PC" root. */
export function localCrumbs(path: string): Crumb[] {
  const top: Crumb = { label: "This PC", path: "", sep: false };
  if (path === "") return [top];
  if (!/^[A-Za-z]:/.test(path)) return posixCrumbs(path);

  const parts = path.split(/[\\/]/).filter(Boolean);
  return [
    top,
    ...parts.map((part, index) => ({
      label: part,
      path: index === 0 ? `${part}\\` : `${parts[0]}\\${parts.slice(1, index + 1).join("\\")}`,
      sep: true,
    })),
  ];
}
