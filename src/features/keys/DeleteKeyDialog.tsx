import { useEffect, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { TriangleAlert } from "lucide-react";
import { errorMessage } from "./api";
import { useKeys } from "./KeysProvider";
import type { SshKey } from "./types";

/** Irreversible, so it asks twice: an explicit acknowledgement plus the
 *  button itself. The fingerprint is shown so you can confirm *which* key is
 *  about to go - file names in `~/.ssh` look alike. */
export function DeleteKeyDialog({
  keyToDelete,
  onClose,
  onDeleted,
}: {
  keyToDelete: SshKey | null;
  onClose: () => void;
  onDeleted?: () => void;
}) {
  const { remove } = useKeys();
  const [includePublic, setIncludePublic] = useState(true);
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (keyToDelete) return;
    setIncludePublic(true);
    setAcknowledged(false);
    setError(null);
    setBusy(false);
  }, [keyToDelete]);

  if (!keyToDelete) return null;

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await remove(keyToDelete, includePublic);
      onDeleted?.();
      onClose();
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(false);
    }
  };

  return (
    <Modal show onHide={onClose} centered backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <TriangleAlert className="text-danger" aria-hidden="true" />
          Delete {keyToDelete.fileName}?
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        {error && <Alert variant="danger">{error}</Alert>}

        <p>
          This permanently removes the key file. Any server, Git host, or
          service that trusts it will stop accepting you, and it cannot be
          recovered without a backup.
        </p>

        <dl className="detail-grid mb-3">
          <div>
            <dt>Algorithm</dt>
            <dd className="font-monospace">{keyToDelete.algorithm}</dd>
          </div>
          <div>
            <dt>Comment</dt>
            <dd>{keyToDelete.comment ?? "None"}</dd>
          </div>
        </dl>

        <div className="mb-3">
          <div className="detail-grid__label">Fingerprint</div>
          <code className="small text-break user-select-auto">
            {keyToDelete.fingerprint ?? "Unknown"}
          </code>
        </div>

        {keyToDelete.publicKeyPath && (
          <Form.Check
            type="checkbox"
            id="delete-public-half"
            className="mb-3"
            label={`Also delete ${keyToDelete.fileName}.pub`}
            checked={includePublic}
            onChange={(event) => setIncludePublic(event.target.checked)}
          />
        )}

        <Form.Check
          type="checkbox"
          id="acknowledge-delete"
          label="I understand this cannot be undone"
          checked={acknowledged}
          onChange={(event) => setAcknowledged(event.target.checked)}
        />
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button variant="danger" onClick={submit} disabled={!acknowledged || busy}>
          {busy && (
            <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
          )}
          {busy ? "Deleting…" : "Delete key"}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
