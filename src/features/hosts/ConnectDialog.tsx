import { useEffect, useRef, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { KeyRound, Plug, ShieldQuestion, TriangleAlert, Usb } from "lucide-react";
import * as api from "./api";
import { errorMessage, hostKeyFingerprint, isUnknownHostKey } from "./api";
import { useHosts } from "./HostsProvider";
import type { HostRow } from "./HostsProvider";
import { describeStage } from "./connectStages";
import { readConnectDetails } from "../settings/preferences";
import type { ConnectionInfo, ConnectStage, PassphraseNeed } from "./types";

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
 *
 * While connecting it says only "Connecting…", unless the detailed-status
 * setting is on - then that same line names the step the backend is on.
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
  /** Read per opening, so flipping the setting applies to the next dialog. */
  const [detailed, setDetailed] = useState(false);
  const [stages, setStages] = useState<ConnectStage[]>([]);
  /** Resolves once the step listener is registered, so an attempt that starts
   *  immediately does not miss its first step. */
  const listening = useRef<Promise<unknown>>(Promise.resolve());

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
    const showSteps = Boolean(host) && readConnectDetails();
    setDetailed(showSteps);
    setStages([]);
    if (!host || !showSteps) {
      listening.current = Promise.resolve();
      return;
    }

    let active = true;
    const registered = api.onConnectProgress(({ hostId, ...stage }) => {
      if (active && hostId === host.id) setStages((previous) => [...previous, stage as ConnectStage]);
    });
    listening.current = registered;
    return () => {
      active = false;
      void registered.then((unlisten) => unlisten());
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

    // Agent and none need nothing from the user, so try straight away.
    if (host.authMethod === "agent" || host.authMethod === "none") {
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
  const isFidoKey = host.authMethod === "publickey" && need?.kind === "hardware";
  // Only once we know the key is locked - and never for one that is not.
  const needsPassphrase =
    host.authMethod === "publickey" && need !== null && need.kind !== "notNeeded" && !isFidoKey;
  const checkingKey = host.authMethod === "publickey" && need === null;

  const attempt = async (trustUnknown: boolean) => {
    setBusy(true);
    setError(null);
    setStages([]);
    try {
      await listening.current;
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

  const canSubmit = !busy && !checkingKey && !isFidoKey && (!needsPassword || password.length > 0);
  /** Methods that connect as soon as the dialog opens. */
  const needsNothing =
    host.authMethod === "agent" ||
    host.authMethod === "none" ||
    (host.authMethod === "publickey" && need?.kind === "notNeeded");

  return (
    <Modal show onHide={() => !busy && onClose()} centered backdrop="static">
      <Modal.Header closeButton={!busy}>
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
          <Form.Group className="mb-3" controlId="connect-password">
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
              disabled={busy}
              autoFocus
              autoComplete="off"
            />
          </Form.Group>
        )}


        {isFidoKey && need?.kind === "hardware" && (
          <Alert variant="info" className="d-flex gap-2">
            <Usb className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>
              <div className="fw-semibold mb-1">
                This key lives on a security token
              </div>
              <code>{host.keyPath ?? "The key"}</code> is a {need.algorithm} key
              - signing requires the hardware authenticator, which this app
              cannot drive directly yet.
              <div className="mt-2">
                To connect with it, switch this host to <strong>SSH agent</strong> auth
                and load the key with <code>ssh-add -K {host.keyPath ?? "path/to/key"}</code>.
                The agent handles the token conversation for you.
              </div>
            </div>
          </Alert>
        )}

        {needsPassphrase && need && (
          <Form.Group className="mb-3" controlId="connect-passphrase">
            <Form.Label className="d-flex align-items-center gap-2">
              <KeyRound className="icon-sm" aria-hidden="true" />
              Key passphrase
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
              disabled={busy}
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
              {need.kind === "unknown" && (
                <>{need.detail} Leave this blank if the key has no passphrase.</>
              )}
            </Form.Text>
          </Form.Group>
        )}

        {!error && !unknownKey && (busy || checkingKey || needsNothing) && (
          <ConnectStatus
            detailed={detailed}
            stages={stages}
            checkingKey={checkingKey ? (host.keyPath ?? "the key") : null}
            // With a field on screen the button already says "Connecting…".
            plain={!needsPassword && !needsPassphrase}
          />
        )}

        {needsPassword && (
          <>
            <Form.Check
              type="checkbox"
              id="remember-password"
              label="Remember this password until I quit"
              checked={remember}
              disabled={busy}
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

/** One status line: "Connecting…", or with the detailed setting on, the step
 *  under way right now. */
function ConnectStatus({
  detailed,
  stages,
  checkingKey,
  plain,
}: {
  detailed: boolean;
  stages: ConnectStage[];
  /** The key being inspected before the attempt, if that is still under way. */
  checkingKey: string | null;
  /** Whether plain mode shows a line at all. */
  plain: boolean;
}) {
  if (!detailed && !plain) return null;

  const latest = stages[stages.length - 1];
  const label = !detailed
    ? "Connecting"
    : latest
      ? describeStage(latest)
      : checkingKey
        ? `Checking whether ${checkingKey} is locked`
        : "Connecting";

  return (
    <p className="text-body-secondary mb-0 d-flex align-items-center gap-2" role="status">
      <Spinner animation="border" size="sm" className="flex-shrink-0" aria-hidden="true" />
      <span className="text-break">{label}…</span>
    </p>
  );
}
