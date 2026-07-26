import { useEffect, useRef, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { FileWarning, FolderPlus, PencilLine, TriangleAlert } from "lucide-react";

import type { RemoteEntry } from "../hosts/types";

/** Names the server will not take, or that would mean something other than what
 *  was typed. Checked here so the refusal is instant and next to the field
 *  rather than a round trip away. */
function nameProblem(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return null; // Not an error yet — just nothing to submit.
  if (trimmed === "." || trimmed === "..") return "That name means a folder that already exists.";
  if (trimmed.includes("/") || trimmed.includes("\\")) {
    return "A name cannot contain a slash — this creates one folder, not a path.";
  }
  if (trimmed.includes(":")) return "A name cannot contain a colon.";
  if (/[\u0000-\u001f\u007f]/.test(trimmed)) {
    return "A name cannot contain control characters.";
  }
  return null;
}

/** Create a folder on the remote host.
 *
 *  Replaces `window.prompt`, which the webview renders as an unstyled browser
 *  dialog announcing `localhost:1420` — it looks like the page is compromised,
 *  and it cannot validate or explain anything.
 */
export function NewFolderDialog({
  show,
  parent,
  busy,
  error,
  onCancel,
  onCreate,
}: {
  show: boolean;
  parent: string | null;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onCreate: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!show) return;
    setName("");
    // Autofocus after the modal's own transition, or the focus is stolen back.
    const timer = setTimeout(() => field.current?.focus(), 120);
    return () => clearTimeout(timer);
  }, [show]);

  const problem = nameProblem(name);
  const canSubmit = name.trim().length > 0 && !problem && !busy;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (canSubmit) onCreate(name.trim());
  };

  return (
    <Modal show={show} onHide={onCancel} centered backdrop="static">
      <Form onSubmit={submit}>
        <Modal.Header closeButton>
          <Modal.Title className="d-flex align-items-center gap-2">
            <FolderPlus className="text-primary" aria-hidden="true" />
            New folder
          </Modal.Title>
        </Modal.Header>

        <Modal.Body>
          <Form.Group controlId="new-folder-name">
            <Form.Label className="small text-body-secondary">
              Created in <span className="font-monospace">{parent ?? "…"}</span>
            </Form.Label>
            <Form.Control
              ref={field}
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Folder name"
              autoComplete="off"
              spellCheck={false}
              isInvalid={Boolean(problem)}
              disabled={busy}
            />
            <Form.Control.Feedback type="invalid">{problem}</Form.Control.Feedback>
          </Form.Group>

          {error && (
            <Alert variant="danger" className="text-prewrap mb-0 mt-3 py-2 small">
              {error}
            </Alert>
          )}
        </Modal.Body>

        <Modal.Footer>
          <Button variant="outline-secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={!canSubmit}>
            {busy && <Spinner animation="border" size="sm" className="me-2" />}
            Create folder
          </Button>
        </Modal.Footer>
      </Form>
    </Modal>
  );
}

/** Confirm deleting one or many remote entries.
 *
 *  Replaces `window.confirm`, which this webview does not reliably block on —
 *  the delete went through whether or not you agreed. Beyond being styled, an
 *  in-app modal is the only version that actually asks.
 */
export function DeleteEntriesDialog({
  targets,
  busy,
  error,
  onCancel,
  onConfirm,
}: {
  targets: RemoteEntry[];
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (targets.length === 0) return null;

  const folders = targets.filter((entry) => entry.kind === "dir").length;
  const single = targets.length === 1 ? targets[0] : null;

  return (
    <Modal show onHide={onCancel} centered backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <TriangleAlert className="text-danger" aria-hidden="true" />
          {single
            ? `Delete ${single.kind === "dir" ? "folder" : "file"}?`
            : `Delete ${targets.length} items?`}
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        {single ? (
          <p className="mb-2">
            <span className="font-monospace">{single.path}</span> will be deleted
            on the server.
          </p>
        ) : (
          <>
            <p className="mb-2">These will be deleted on the server:</p>
            <ul className="files-confirm__list font-monospace small">
              {targets.slice(0, 12).map((entry) => (
                <li key={entry.path}>{entry.path}</li>
              ))}
              {targets.length > 12 && (
                <li className="text-body-secondary">
                  …and {targets.length - 12} more
                </li>
              )}
            </ul>
          </>
        )}

        <p className="text-body-secondary mb-0">
          {folders > 0
            ? "This cannot be undone, and there is no recycle bin on the server. A folder is only removed if it is already empty."
            : "This cannot be undone, and there is no recycle bin on the server."}
        </p>

        {error && (
          <Alert variant="danger" className="text-prewrap mb-0 mt-3 py-2 small">
            {error}
          </Alert>
        )}
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button variant="danger" onClick={onConfirm} disabled={busy}>
          {busy && <Spinner animation="border" size="sm" className="me-2" />}
          {single ? "Delete" : `Delete ${targets.length}`}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}

/** Rename an entry in place. The extension is preselected out of the initial
 *  selection, so typing replaces the stem and keeps `.tar.gz` intact. */
export function RenameDialog({
  entry,
  busy,
  error,
  onCancel,
  onRename,
}: {
  entry: RemoteEntry | null;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onRename: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!entry) return;
    setName(entry.name);
    const timer = setTimeout(() => {
      const input = field.current;
      if (!input) return;
      input.focus();
      // Select the stem only: renaming almost never means changing the suffix.
      const dot = entry.name.lastIndexOf(".");
      input.setSelectionRange(0, dot > 0 ? dot : entry.name.length);
    }, 120);
    return () => clearTimeout(timer);
  }, [entry]);

  if (!entry) return null;

  const problem = nameProblem(name);
  const changed = name.trim() !== entry.name;
  const canSubmit = name.trim().length > 0 && !problem && changed && !busy;

  return (
    <Modal show onHide={onCancel} centered backdrop="static">
      <Form
        onSubmit={(event) => {
          event.preventDefault();
          if (canSubmit) onRename(name.trim());
        }}
      >
        <Modal.Header closeButton>
          <Modal.Title className="d-flex align-items-center gap-2">
            <PencilLine className="text-primary" aria-hidden="true" />
            Rename
          </Modal.Title>
        </Modal.Header>

        <Modal.Body>
          <Form.Group controlId="rename-name">
            <Form.Label className="small text-body-secondary">
              In <span className="font-monospace">{parentOf(entry.path)}</span>
            </Form.Label>
            <Form.Control
              ref={field}
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              isInvalid={Boolean(problem)}
              disabled={busy}
            />
            <Form.Control.Feedback type="invalid">{problem}</Form.Control.Feedback>
          </Form.Group>

          {error && (
            <Alert variant="danger" className="text-prewrap mb-0 mt-3 py-2 small">
              {error}
            </Alert>
          )}
        </Modal.Body>

        <Modal.Footer>
          <Button variant="outline-secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={!canSubmit}>
            {busy && <Spinner animation="border" size="sm" className="me-2" />}
            Rename
          </Button>
        </Modal.Footer>
      </Form>
    </Modal>
  );
}

/** What the user chose for one naming clash. */
export type ConflictChoice = "overwrite" | "keepBoth" | "skip";

export type ConflictPrompt = {
  /** The name already taken at the destination. */
  name: string;
  destination: string;
  /** How many clashes are left including this one, for the apply-to-all copy. */
  remaining: number;
};

/** Ask what to do about a name that is already taken.
 *
 *  Shown one clash at a time rather than as a list: the decision is per file,
 *  and "apply to the rest" covers the case where it is not. Without that
 *  checkbox a recursive transfer could ask fifty times.
 */
export function ConflictDialog({
  prompt,
  onResolve,
  onCancel,
}: {
  prompt: ConflictPrompt | null;
  onResolve: (choice: ConflictChoice, applyToAll: boolean) => void;
  onCancel: () => void;
}) {
  const [applyToAll, setApplyToAll] = useState(false);

  useEffect(() => {
    setApplyToAll(false);
  }, [prompt?.name, prompt?.destination]);

  if (!prompt) return null;

  return (
    <Modal show onHide={onCancel} centered backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <FileWarning className="text-warning" aria-hidden="true" />
          Already exists
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        <p className="mb-2">
          <span className="font-monospace">{prompt.name}</span> is already in{" "}
          <span className="font-monospace">{prompt.destination}</span>.
        </p>
        <p className="text-body-secondary small mb-3">
          <b>Keep both</b> saves the incoming copy as
          {" "}<span className="font-monospace">{suffixed(prompt.name)}</span>.
          {" "}<b>Overwrite</b> cannot be undone.
        </p>

        {prompt.remaining > 1 && (
          <Form.Check
            id="conflict-apply-all"
            checked={applyToAll}
            onChange={(event) => setApplyToAll(event.target.checked)}
            label={`Do this for all ${prompt.remaining} conflicts`}
          />
        )}
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={() => onResolve("skip", applyToAll)}>
          Skip
        </Button>
        <Button variant="outline-primary" onClick={() => onResolve("keepBoth", applyToAll)}>
          Keep both
        </Button>
        <Button variant="danger" onClick={() => onResolve("overwrite", applyToAll)}>
          Overwrite
        </Button>
      </Modal.Footer>
    </Modal>
  );
}

function parentOf(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut <= 0 ? "/" : path.slice(0, cut);
}

/** Mirrors the backend's "keep both" scheme, so the preview matches reality. */
function suffixed(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? `${name.slice(0, dot)} (1)${name.slice(dot)}` : `${name} (1)`;
}
