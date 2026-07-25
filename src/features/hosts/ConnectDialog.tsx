import { useEffect, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { KeyRound, Plug, ShieldQuestion, TriangleAlert } from "lucide-react";
import { errorMessage, hostKeyFingerprint, isUnknownHostKey } from "./api";
import { useHosts } from "./HostsProvider";
import type { HostRow } from "./HostsProvider";
import type { ConnectionInfo } from "./types";

/**
 * Collects whatever the chosen auth method needs, then connects.
 *
 * Two things make this more than a password box:
 *
 *  1. An unknown host key stops the connection *before* a password is sent,
 *     and this dialog is where the fingerprint is shown and accepted. A
 *     changed key is not offered as something to click through.
 *  2. "Remember" means until the app quits, and says so — no keychain is
 *     involved, and implying otherwise would be a lie about where a password
 *     ended up.
 */
export function ConnectDialog({
  host,
  onClose,
  onConnected,
}: {
  host: HostRow | null;
  onClose: () => void;
  onConnected?: (info: ConnectionInfo) => void;
}) {
  const { connect } = useHosts();

  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Set when the server offered a key we have never seen. */
  const [unknownKey, setUnknownKey] = useState<string | null>(null);

  useEffect(() => {
    if (host) {
      // Agent auth needs nothing from the user, so try straight away.
      if (host.authMethod === "agent") void attempt(false);
      return;
    }
    setPassword("");
    setRemember(false);
    setBusy(false);
    setError(null);
    setUnknownKey(null);
    // `attempt` is stable for a given host; re-running on every render would
    // reconnect in a loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host]);

  if (!host) return null;

  const needsPassword = host.authMethod === "password";
  const needsPassphrase = host.authMethod === "publickey";

  const attempt = async (trustUnknown: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const info = await connect(host.id, {
        password: password || null,
        remember: remember && needsPassword,
        trustUnknown,
      });
      setPassword("");
      onConnected?.(info);
      onClose();
    } catch (caught) {
      if (isUnknownHostKey(caught)) {
        // Ask about the key rather than reporting a failure: the password was
        // never sent, so nothing has leaked and the retry is safe.
        setUnknownKey(hostKeyFingerprint(caught) ?? "unknown fingerprint");
        setError(null);
      } else {
        setError(errorMessage(caught));
        setUnknownKey(null);
      }
      setBusy(false);
    }
  };

  const canSubmit = !busy && (!needsPassword || password.length > 0);

  return (
    <Modal show onHide={onClose} centered backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <Plug aria-hidden="true" />
          Connect to {host.label}
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        <p className="text-body-secondary font-monospace small">
          {host.username}@{host.hostname}:{host.port}
        </p>

        {error && <Alert variant="danger" className="text-prewrap">{error}</Alert>}

        {unknownKey && (
          <Alert variant="warning" className="d-flex gap-2">
            <ShieldQuestion className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>
              <div className="fw-semibold mb-1">This host is not yet known.</div>
              It identifies itself with:
              <div className="public-key-box user-select-auto my-2">{unknownKey}</div>
              Nothing has been sent yet. If that fingerprint matches what the
              server should have, trust it — it will be written to your
              <code> known_hosts </code> and checked automatically from now on.
            </div>
          </Alert>
        )}

        {needsPassword && (
          <Form.Group className="mb-3">
            <Form.Label>Password for {host.username}</Form.Label>
            <Form.Control
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && canSubmit) {
                  void attempt(Boolean(unknownKey));
                }
              }}
              autoFocus
              autoComplete="off"
            />
          </Form.Group>
        )}

        {needsPassphrase && (
          <Form.Group className="mb-3">
            <Form.Label className="d-flex align-items-center gap-2">
              <KeyRound className="icon-sm" aria-hidden="true" />
              Key passphrase
            </Form.Label>
            <Form.Control
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoFocus
              autoComplete="off"
            />
            <Form.Text className="text-body-secondary">
              Leave blank if <code>{host.keyPath ?? "the key"}</code> has no
              passphrase.
            </Form.Text>
          </Form.Group>
        )}

        {host.authMethod === "agent" && !error && !unknownKey && (
          <p className="text-body-secondary mb-0">
            Offering the keys held by your SSH agent…
          </p>
        )}

        {needsPassword && (
          <>
            <Form.Check
              type="checkbox"
              id="remember-password"
              label="Remember this password until I quit"
              checked={remember}
              onChange={(event) => setRemember(event.target.checked)}
            />
            {remember && (
              <Alert variant="secondary" className="mt-2 mb-0 d-flex gap-2 py-2 small">
                <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
                <div>
                  Held in memory only — not in your keychain, and not on disk.
                  Quitting ParolaSSH forgets it.
                </div>
              </Alert>
            )}
          </>
        )}
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button
          variant={unknownKey ? "warning" : "primary"}
          onClick={() => attempt(Boolean(unknownKey))}
          disabled={!canSubmit}
        >
          {busy && (
            <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
          )}
          {busy ? "Connecting…" : unknownKey ? "Trust and connect" : "Connect"}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
