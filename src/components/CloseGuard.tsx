import { useEffect, useState } from "react";
import { Button, Modal } from "react-bootstrap";
import { TriangleAlert } from "lucide-react";
import * as api from "../features/hosts/api";
import { useHosts } from "../features/hosts/HostsProvider";
import { appWindow } from "../lib/appWindow";

/**
 * Decides what closing the window means. With no live sessions it just closes;
 * with sessions up the close is intercepted so the user can pick between
 * quitting, hiding to the tray with sessions alive, or staying.
 *
 * The Rust registry is asked directly rather than trusting cached UI state, so
 * a session that died since the last heartbeat does not block the exit.
 */
export function CloseGuard() {
  const { hosts } = useHosts();
  const [connectedIds, setConnectedIds] = useState<string[] | null>(null);

  useEffect(() => {
    const unlisten = appWindow.onCloseRequested(async (event) => {
      let connected: string[] = [];
      try {
        connected = await api.connectedHosts();
      } catch {
        // If the backend cannot even answer, there is nothing worth guarding.
      }
      if (connected.length === 0) return;
      event.preventDefault();
      setConnectedIds(connected);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  if (!connectedIds) return null;

  const labels = connectedIds.map(
    (id) => hosts.find((host) => host.id === id)?.label ?? id,
  );

  const quit = () => {
    // Skips the close-requested round trip; the Rust exit handler still closes
    // every session cleanly.
    void appWindow.destroy();
  };

  const hideToTray = () => {
    setConnectedIds(null);
    void appWindow.hide();
  };

  return (
    <Modal show onHide={() => setConnectedIds(null)} centered backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <TriangleAlert className="text-warning" aria-hidden="true" />
          {connectedIds.length === 1
            ? "1 connection is still active"
            : `${connectedIds.length} connections are still active`}
        </Modal.Title>
      </Modal.Header>
      <Modal.Body>
        <p>
          You are still connected to <strong>{labels.join(", ")}</strong>.
        </p>
        <p className="mb-0 text-body-secondary">
          Quitting disconnects every session. Minimizing to the tray keeps
          them alive — ParolaSSH stays in the system tray, ready to reopen.
        </p>
      </Modal.Body>
      <Modal.Footer>
        <Button
          variant="outline-secondary"
          onClick={() => setConnectedIds(null)}
        >
          Cancel
        </Button>
        <Button variant="outline-primary" onClick={hideToTray}>
          Minimize to tray
        </Button>
        <Button variant="danger" onClick={quit}>
          Disconnect &amp; quit
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
