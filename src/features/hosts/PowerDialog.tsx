import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button, Col, Form, Modal, Row, Spinner } from "react-bootstrap";
import {
  CircleCheck,
  CircleX,
  Power,
  RotateCcw,
  ShieldCheck,
  ShieldAlert,
  TriangleAlert,
  XCircle,
} from "lucide-react";
import * as api from "./api";
import { errorMessage } from "./api";
import { useHosts } from "./HostsProvider";
import type { HostRow } from "./HostsProvider";
import {
  ELEVATION_LABELS,
  OS_LABELS,
  type PowerAction,
  type PowerOutcome,
  type PowerPlan,
  type PowerRequest,
} from "./types";

/** Offered delays. Anything else can be typed into the minutes field. */
const PRESETS = [
  { minutes: 0, label: "Now" },
  { minutes: 1, label: "1 min" },
  { minutes: 5, label: "5 min" },
  { minutes: 15, label: "15 min" },
  { minutes: 60, label: "1 hour" },
];

/**
 * Shut down or reboot a connected host.
 *
 * The dialog leans on the Rust side for two things it refuses to guess at:
 * how this account elevates (sudo, an already-elevated Windows token, or not
 * at all), and the literal command that will run. Both are shown before the
 * button is armed — a remote reboot is not something to trigger from a button
 * whose behaviour is a mystery.
 */
export function PowerDialog({
  host,
  onClose,
}: {
  host: HostRow | null;
  onClose: () => void;
}) {
  const { getConnection, power } = useHosts();
  const connection = host ? getConnection(host.id) : undefined;

  const [action, setAction] = useState<PowerAction>("reboot");
  const [minutes, setMinutes] = useState(0);
  const [force, setForce] = useState(false);
  const [message, setMessage] = useState("");
  const [password, setPassword] = useState("");
  /** Reuse the password this session logged in with, rather than retyping it. */
  const [reuseLogin, setReuseLogin] = useState(true);
  const [confirmed, setConfirmed] = useState(false);

  const [plan, setPlan] = useState<PowerPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<PowerOutcome | null>(null);

  const request: PowerRequest = {
    action,
    delayMinutes: action === "cancel" ? 0 : minutes,
    force,
    message: message.trim() || null,
  };

  useEffect(() => {
    if (host) return;
    setAction("reboot");
    setMinutes(0);
    setForce(false);
    setMessage("");
    setPassword("");
    setReuseLogin(true);
    setConfirmed(false);
    setPlan(null);
    setPlanError(null);
    setError(null);
    setOutcome(null);
    setBusy(false);
  }, [host]);

  const hostId = host?.id;
  const requestKey = JSON.stringify(request);

  // Ask the Rust side what it would run, every time the request changes.
  const refreshPlan = useCallback(async () => {
    if (!hostId) return;
    try {
      setPlan(await api.previewPower(hostId, JSON.parse(requestKey) as PowerRequest));
      setPlanError(null);
    } catch (caught) {
      setPlan(null);
      setPlanError(errorMessage(caught));
    }
  }, [hostId, requestKey]);

  useEffect(() => {
    if (!hostId || outcome) return;
    void refreshPlan();
  }, [hostId, outcome, refreshPlan]);

  // Any change to what will happen retracts the confirmation.
  useEffect(() => setConfirmed(false), [requestKey]);

  if (!host) return null;

  if (!connection) {
    return (
      <Modal show onHide={onClose} centered>
        <Modal.Header closeButton>
          <Modal.Title>Power — {host.label}</Modal.Title>
        </Modal.Header>
        <Modal.Body>
          <Alert variant="warning" className="mb-0">
            Connect to this host first. The power options depend on what the
            remote machine is and how this account can elevate.
          </Alert>
        </Modal.Body>
        <Modal.Footer>
          <Button variant="outline-secondary" onClick={onClose}>
            Close
          </Button>
        </Modal.Footer>
      </Modal>
    );
  }

  const { elevation, elevationExplanation, os, osDetail, user } = connection;
  const blocked = elevation.kind === "unavailable";
  const needsPassword = plan?.needsPassword ?? elevation.kind === "sudoPassword";
  const destructive = action !== "cancel";

  // The session already holds the login password on a password-auth host, and
  // on Unix it is nearly always the same string sudo wants. Offering it beats
  // making people type it twice — but it stays overridable, because sudo may
  // be configured to run as a different account.
  const canReuse = needsPassword && connection.hasLoginPassword;
  const usingLogin = canReuse && reuseLogin;

  const canRun =
    !busy &&
    !blocked &&
    plan !== null &&
    (!destructive || confirmed) &&
    (!needsPassword || usingLogin || password.length > 0);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      // Sending null tells the Rust side to fall back to the session's own
      // login password; it never travels back to the webview to get here.
      setOutcome(await power(host.id, request, usingLogin ? null : password || null));
      setPassword("");
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal show onHide={onClose} centered size="lg" backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <Power aria-hidden="true" />
          Power — {host.label}
        </Modal.Title>
      </Modal.Header>

      {outcome ? (
        <>
          <Modal.Body>
            <Alert
              variant={outcome.succeeded ? "success" : "danger"}
              className="d-flex gap-2"
            >
              {outcome.succeeded ? (
                <CircleCheck className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
              ) : (
                <CircleX className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
              )}
              <div>
                <div className="fw-semibold">{outcome.summary}</div>
                <div className="text-prewrap">{outcome.message}</div>
              </div>
            </Alert>

            <div className="detail-grid__label">Command that ran</div>
            <div className="public-key-box user-select-auto">{outcome.command}</div>

            {(outcome.stdout.trim() || outcome.stderr.trim()) && (
              <>
                <div className="detail-grid__label mt-3">Output</div>
                <pre className="command-output user-select-auto mb-0">
                  {[outcome.stdout, outcome.stderr].filter(Boolean).join("\n").trim()}
                </pre>
              </>
            )}
          </Modal.Body>
          <Modal.Footer>
            <Button variant="outline-secondary" onClick={() => setOutcome(null)}>
              Back
            </Button>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </Modal.Footer>
        </>
      ) : (
        <>
          <Modal.Body>
            {error && <Alert variant="danger" className="text-prewrap">{error}</Alert>}

            {/* What we are talking to, and how it will elevate. */}
            <div className="privilege-card mb-3">
              <div className="d-flex flex-wrap align-items-center gap-2 mb-2">
                <Badge bg="secondary">{OS_LABELS[os]}</Badge>
                <span className="text-body-secondary small font-monospace">
                  {osDetail}
                </span>
                {user && (
                  <span className="text-body-secondary small">as {user}</span>
                )}
              </div>

              <div className="d-flex gap-2">
                {blocked ? (
                  <ShieldAlert
                    className="icon-sm flex-shrink-0 mt-1 text-danger"
                    aria-hidden="true"
                  />
                ) : (
                  <ShieldCheck
                    className="icon-sm flex-shrink-0 mt-1 text-success"
                    aria-hidden="true"
                  />
                )}
                <div>
                  <div className="fw-semibold">
                    {ELEVATION_LABELS[elevation.kind]}
                  </div>
                  <div className="text-body-secondary small">
                    {elevationExplanation}
                  </div>
                </div>
              </div>
            </div>

            {blocked && (
              <Alert variant="danger" className="d-flex gap-2">
                <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
                <div>
                  {elevation.kind === "unavailable" && elevation.reason}
                </div>
              </Alert>
            )}

            <Form.Group className="mb-3">
              <Form.Label className="fw-semibold">Action</Form.Label>
              <div className="algorithm-choices">
                <ActionChoice
                  active={action === "reboot"}
                  onClick={() => setAction("reboot")}
                  Icon={RotateCcw}
                  name="Reboot"
                  hint="Restart the machine"
                />
                <ActionChoice
                  active={action === "shutdown"}
                  onClick={() => setAction("shutdown")}
                  Icon={Power}
                  name="Shut down"
                  hint="Power off — you will need physical or IPMI access to bring it back"
                />
                {connection.supportsCancel && (
                  <ActionChoice
                    active={action === "cancel"}
                    onClick={() => setAction("cancel")}
                    Icon={XCircle}
                    name="Cancel"
                    hint="Call off a scheduled shutdown"
                  />
                )}
              </div>
            </Form.Group>

            {action !== "cancel" && (
              <Row className="g-3 mb-3">
                <Col sm={7}>
                  <Form.Label className="fw-semibold">When</Form.Label>
                  <div className="d-flex flex-wrap gap-1">
                    {PRESETS.map((preset) => (
                      <Button
                        key={preset.minutes}
                        size="sm"
                        variant={
                          minutes === preset.minutes ? "primary" : "outline-secondary"
                        }
                        onClick={() => setMinutes(preset.minutes)}
                      >
                        {preset.label}
                      </Button>
                    ))}
                  </div>
                </Col>
                <Col sm={5}>
                  <Form.Label>Or in… (minutes)</Form.Label>
                  <Form.Control
                    type="number"
                    min={0}
                    value={minutes}
                    onChange={(event) =>
                      setMinutes(Math.max(0, Number(event.target.value) || 0))
                    }
                  />
                </Col>
              </Row>
            )}

            {action !== "cancel" && (
              <Form.Group className="mb-3">
                <Form.Label>Message to logged-in users</Form.Label>
                <Form.Control
                  value={message}
                  onChange={(event) => setMessage(event.target.value)}
                  placeholder="Patching — back in five minutes"
                />
              </Form.Group>
            )}

            {connection.supportsForce && action !== "cancel" && (
              <Form.Check
                type="checkbox"
                id="power-force"
                className="mb-3"
                label="Force applications to close without saving (Windows /f)"
                checked={force}
                onChange={(event) => setForce(event.target.checked)}
              />
            )}

            {needsPassword && (
              <div className="mb-3">
                {canReuse && (
                  <Form.Check
                    type="checkbox"
                    id="power-reuse-login"
                    className="mb-2"
                    label={`Use the password I logged in with as ${user}`}
                    checked={reuseLogin}
                    onChange={(event) => setReuseLogin(event.target.checked)}
                  />
                )}

                {!usingLogin && (
                  <Form.Group>
                    <Form.Label>sudo password for {user}</Form.Label>
                    <Form.Control
                      type="password"
                      value={password}
                      onChange={(event) => setPassword(event.target.value)}
                      autoComplete="off"
                      autoFocus={canReuse}
                    />
                  </Form.Group>
                )}

                <Form.Text className="text-body-secondary">
                  Sent to <code>sudo -S</code> over the existing encrypted
                  channel, never as part of the command line — so it stays out
                  of the remote process list.
                </Form.Text>
              </div>
            )}

            {/* The literal command. */}
            {planError && <Alert variant="warning">{planError}</Alert>}
            {plan && (
              <>
                <div className="detail-grid__label">Command that will run</div>
                <div className="public-key-box user-select-auto">{plan.command}</div>
              </>
            )}

            {destructive && plan && !blocked && (
              <Form.Check
                type="checkbox"
                id="power-confirm"
                className="mt-3"
                label={`I want to ${plan.summary.toLowerCase()} on ${host.hostname}`}
                checked={confirmed}
                onChange={(event) => setConfirmed(event.target.checked)}
              />
            )}
          </Modal.Body>

          <Modal.Footer>
            <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant={destructive ? "danger" : "primary"}
              onClick={run}
              disabled={!canRun}
            >
              {busy && (
                <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
              )}
              {busy ? "Sending…" : plan?.summary ?? "Run"}
            </Button>
          </Modal.Footer>
        </>
      )}
    </Modal>
  );
}

function ActionChoice({
  active,
  onClick,
  Icon,
  name,
  hint,
}: {
  active: boolean;
  onClick: () => void;
  Icon: typeof Power;
  name: string;
  hint: string;
}) {
  return (
    <button
      type="button"
      className={`algorithm-choice${active ? " is-active" : ""}`}
      onClick={onClick}
      aria-pressed={active}
    >
      <span className="algorithm-choice__name d-flex align-items-center gap-2">
        <Icon className="icon-sm" aria-hidden="true" />
        {name}
      </span>
      <span className="algorithm-choice__hint">{hint}</span>
    </button>
  );
}
