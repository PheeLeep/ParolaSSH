import { useEffect, useState } from "react";
import { Alert, Button, Modal, Spinner } from "react-bootstrap";
import { TriangleAlert } from "lucide-react";
import { errorMessage } from "./api";
import { useHosts } from "./HostsProvider";
import type { HostRow } from "./HostsProvider";

/** Confirm removing a saved connection. Lighter than the key deletion dialog -
 *  this removes a bookmark, not a credential - but the address is spelled out,
 *  because labels in a long list look alike. */
export function DeleteHostDialog({
  host,
  onClose,
  onDeleted,
}: {
  host: HostRow | null;
  onClose: () => void;
  onDeleted?: () => void;
}) {
  const { remove } = useHosts();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (host) return;
    setBusy(false);
    setError(null);
  }, [host]);

  if (!host) return null;

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await remove(host.id);
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
          Remove {host.label}?
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        {error && <Alert variant="danger">{error}</Alert>}

        <p>
          This removes the saved connection from ParolaSSH. The machine itself
          is untouched, and no keys are deleted.
        </p>

        <dl className="detail-grid mb-0">
          <div>
            <dt>Address</dt>
            <dd className="font-monospace">
              {host.username}@{host.hostname}:{host.port}
            </dd>
          </div>
          <div>
            <dt>Group</dt>
            <dd>{host.group}</dd>
          </div>
        </dl>

        {host.status === "connected" && (
          <Alert variant="warning" className="mt-3 mb-0 d-flex gap-2 py-2 small">
            <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>
              This host is connected. Removing it will close the session and any
              open terminal.
            </div>
          </Alert>
        )}
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button variant="danger" onClick={submit} disabled={busy}>
          {busy && (
            <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
          )}
          {busy ? "Removing…" : "Remove connection"}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
