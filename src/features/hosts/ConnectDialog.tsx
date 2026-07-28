import { useEffect, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { KeyRound, Plug, ShieldQuestion, TriangleAlert, Usb } from "lucide-react";
import * as api from "./api";
import { errorMessage, hostKeyFingerprint, isUnknownHostKey } from "./api";
import { useHosts } from "./HostsProvider";
import type { HostRow } from "./HostsProvider";
import type { ConnectionInfo, PassphraseNeed } from "./types";

/**
 * Collects whatever the chosen auth method needs, then connects.
 *
 * Three things make this more than a password box:
 *
 *  1. An unknown host key stops the connection before a password is sent, and
 *     this is where the fingerprint is shown and accepted. A changed key is
 *     never offered as click-through.
 *  2. "Remember" means until the app quits, and says so - no keychain.
 *  3. A key connection asks the Rust side whether the key is really locked
 *     before showing a passphrase box, so an unencrypted key connects as
 *     directly as agent auth.
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
  /** What the key on disk needs, once the Rust side has looked at it. */
  const [need, setNeed] = useState<PassphraseNeed | null>(null);

  // The tray blinks from the moment we start asking, not from the submit:
  // the attempt began when this opened. Cleanup covers cancel, close, and
  // navigating away, so there is one path back out.
  useEffect(() => {
    if (!host) return;
    void api.setConnectPending(host.id, true);
    return () => {
      void api.setConnectPending(host.id, false);
    };
  }, [host]);

  useEffect(() => {
    if (!host) {
      setPassword("");
      setRemember(false);
      setBusy(false);
      setError(null);
      setUnknownKey(null);
      setNeed(null);
      return;
    }

    // Agent auth needs nothing from the user, so try straight away.
    if (host.authMethod === "agent") {
      void attempt(false);
      return;
    }
    if (host.authMethod !== "publickey") return;

    let cancelled = false;
    void (async () => {
      let answer: PassphraseNeed;
      try {
        answer = await api.hostKeyPassphraseNeed(host.id);
      } catch (caught) {
        // Could not tell, so ask; the attempt reports the real problem.
        answer = { kind: "unknown", detail: errorMessage(caught) };
      }
      if (cancelled) return;
      setNeed(answer);
      // Nothing to type: connect rather than showing an empty box.
      if (answer.kind === "notNeeded") void attempt(false);
    })();

    return () => {
      cancelled = true;
    };
    // `attempt` is stable for a given host; re-running on every render would
    // reconnect in a loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host]);

  if (!host) return null;

  const needsPassword = host.authMethod === "password";
  // Only once we know the key is locked - and never for one that is not.
  const needsPassphrase =
    host.authMethod === "publickey" && need !== null && need.kind !== "notNeeded";
  const checkingKey = host.authMethod === "publickey" && need === null;

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
        // The password was never sent, so asking again is safe.
        setUnknownKey(hostKeyFingerprint(caught) ?? "unknown fingerprint");
        setError(null);
      } else {
        setError(errorMessage(caught));
        setUnknownKey(null);
      }
      setBusy(false);
    }
  };

  const canSubmit = !busy && !checkingKey && (!needsPassword || password.length > 0);

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
              server should have, trust it - it will be written to your
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

        {checkingKey && !error && (
          <p className="text-body-secondary mb-0">
            Checking whether <code>{host.keyPath ?? "the key"}</code> is
            locked…
          </p>
        )}

        {needsPassphrase && need && (
          <Form.Group className="mb-3">
            <Form.Label className="d-flex align-items-center gap-2">
              {need.kind === "hardware" ? (
                <Usb className="icon-sm" aria-hidden="true" />
              ) : (
                <KeyRound className="icon-sm" aria-hidden="true" />
              )}
              {need.kind === "hardware"
                ? "Security key PIN or passphrase"
                : "Key passphrase"}
            </Form.Label>
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
            <Form.Text className="text-body-secondary">
              {need.kind === "required" && (
                <>
                  <code>{host.keyPath ?? "The key"}</code> is encrypted - this
                  unlocks it, and is never stored.
                </>
              )}
              {need.kind === "hardware" && (
                <>
                  {need.algorithm} lives on a token: it may want a PIN here and
                  a touch on the device itself.
                </>
              )}
              {need.kind === "unknown" && (
                <>{need.detail} Leave this blank if the key has no passphrase.</>
              )}
            </Form.Text>
          </Form.Group>
        )}

        {(host.authMethod === "agent" ||
          host.authMethod === "none" ||
          (host.authMethod === "publickey" && need?.kind === "notNeeded")) &&
          !error &&
          !unknownKey && (
            <p className="text-body-secondary mb-0">
              {host.authMethod === "agent"
                ? "Offering the keys held by your SSH agent…"
                : host.authMethod === "none"
                  ? "Sending no credential - this host identifies you before SSH begins…"
                  : "The key is not encrypted, so there is nothing to unlock - connecting…"}
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
                  Held in memory only - not in your keychain, and not on disk.
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
