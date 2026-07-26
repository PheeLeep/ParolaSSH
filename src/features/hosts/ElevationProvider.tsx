import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { Alert, Button, Form, Modal } from "react-bootstrap";
import { ShieldAlert, ShieldCheck, TriangleAlert } from "lucide-react";
import { useHosts } from "./HostsProvider";
import { ELEVATION_LABELS } from "./types";

/**
 * One prompt for every privileged action, asked at the moment of elevating.
 *
 * Before this existed, each pane grew its own sudo password box and
 * password-less elevation — root, NOPASSWD sudo, a Windows administrator
 * token — ran with no confirmation at all. That is the wrong way round: the
 * thing worth confirming is that a command is about to run *as root on
 * someone else's machine*, and whether a password is involved is an accident
 * of how that host's sudo is configured.
 *
 * So the prompt always appears, and adapts:
 *
 *  - `sudoPassword` — asks for the account password, offering the one this
 *    session logged in with rather than making it be typed twice.
 *  - `notNeeded` / `sudoNoPassword` / `windowsAdminToken` — a consent step
 *    showing the literal command. Nothing to type; something to agree to.
 *  - `unavailable` — explains why there is no route to root and offers no
 *    button that pretends otherwise.
 *
 * The password is handed back to the caller and never held here.
 */

export interface ElevationRequest {
  hostId: string;
  /** What the user asked for: "Reboot web-01", "Restart cron.service". */
  summary: string;
  /** The literal command, when the caller has already previewed it. */
  command?: string | null;
  /** True when the command interrupts or changes the machine. */
  destructive?: boolean;
  /**
   * Set when the action is still useful unelevated — the audit's read-only
   * checks, for instance. Becomes a second button, labelled with this text.
   */
  unprivilegedLabel?: string;
}

export type ElevationGrant =
  /** `password` is null when sudo needs none, or when the session's own
   *  login password should be used — it never travels through the webview. */
  | { outcome: "granted"; password: string | null }
  /** The user chose to continue without elevating. */
  | { outcome: "unprivileged" }
  | { outcome: "cancelled" };

type RequestElevation = (request: ElevationRequest) => Promise<ElevationGrant>;

const ElevationContext = createContext<RequestElevation | null>(null);

export function ElevationProvider({ children }: { children: React.ReactNode }) {
  const [pending, setPending] = useState<ElevationRequest | null>(null);
  const settleRef = useRef<((grant: ElevationGrant) => void) | null>(null);

  const requestElevation = useCallback<RequestElevation>((request) => {
    // A second request while one is open would strand the first promise, and
    // a caller awaiting a prompt that vanished would hang forever.
    settleRef.current?.({ outcome: "cancelled" });
    setPending(request);
    return new Promise<ElevationGrant>((resolve) => {
      settleRef.current = resolve;
    });
  }, []);

  const settle = useCallback((grant: ElevationGrant) => {
    settleRef.current?.(grant);
    settleRef.current = null;
    setPending(null);
  }, []);

  return (
    <ElevationContext.Provider value={requestElevation}>
      {children}
      <ElevationPrompt request={pending} onSettle={settle} />
    </ElevationContext.Provider>
  );
}

/** Ask to elevate. Resolves once the user has answered. */
export function useElevation(): RequestElevation {
  const request = useContext(ElevationContext);
  if (!request) {
    throw new Error("useElevation must be used inside an ElevationProvider");
  }
  return request;
}

function ElevationPrompt({
  request,
  onSettle,
}: {
  request: ElevationRequest | null;
  onSettle: (grant: ElevationGrant) => void;
}) {
  const { getConnection } = useHosts();
  const connection = request ? getConnection(request.hostId) : undefined;

  const [password, setPassword] = useState("");
  /** Reuse the password this session logged in with, rather than retyping. */
  const [reuseLogin, setReuseLogin] = useState(true);

  useEffect(() => {
    setPassword("");
    setReuseLogin(true);
  }, [request]);

  if (!request) return null;

  const cancel = () => onSettle({ outcome: "cancelled" });

  // Rendered above whatever opened it — these prompts are raised from inside
  // other modals, and Bootstrap's default stacking would bury this one.
  const stacked = { style: { zIndex: 1075 }, backdropClassName: "elevation-backdrop" };

  if (!connection) {
    return (
      <Modal show onHide={cancel} centered {...stacked}>
        <Modal.Header closeButton>
          <Modal.Title>Not connected</Modal.Title>
        </Modal.Header>
        <Modal.Body>
          <Alert variant="warning" className="mb-0">
            That host is no longer connected, so nothing can be elevated on it.
          </Alert>
        </Modal.Body>
        <Modal.Footer>
          <Button variant="outline-secondary" onClick={cancel}>
            Close
          </Button>
        </Modal.Footer>
      </Modal>
    );
  }

  const { elevation, elevationExplanation, user } = connection;
  const blocked = elevation.kind === "unavailable";
  const needsPassword = elevation.kind === "sudoPassword";
  const canReuse = needsPassword && connection.hasLoginPassword;
  const usingLogin = canReuse && reuseLogin;
  const canGrant =
    !blocked && (!needsPassword || usingLogin || password.length > 0);

  const grant = () =>
    onSettle({
      outcome: "granted",
      // Null tells the Rust side to fall back to the session's own login
      // password; it never travels back to the webview to get here.
      password: usingLogin ? null : password || null,
    });

  return (
    <Modal show onHide={cancel} centered backdrop="static" {...stacked}>
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          {blocked ? (
            <ShieldAlert aria-hidden="true" />
          ) : (
            <ShieldCheck aria-hidden="true" />
          )}
          Elevate — {request.summary}
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        <Alert
          variant={blocked ? "danger" : request.destructive ? "warning" : "secondary"}
          className="d-flex gap-2"
        >
          <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
          <div>
            <div className="fw-semibold mb-1">
              {blocked
                ? "This account cannot elevate on this host."
                : elevation.kind === "windowsAdminToken"
                  ? "This runs with the full Administrator token."
                  : elevation.kind === "notNeeded"
                    ? `This runs as root — ${user} already is root.`
                    : "This runs with root privileges."}
            </div>
            {elevation.kind === "unavailable" ? elevation.reason : elevationExplanation}
          </div>
        </Alert>

        <dl className="detail-grid mb-3">
          <div>
            <dt>Account</dt>
            <dd className="font-monospace small">{user}</dd>
          </div>
          <div>
            <dt>Elevation</dt>
            <dd>{ELEVATION_LABELS[elevation.kind]}</dd>
          </div>
        </dl>

        {request.command && (
          <>
            <div className="detail-grid__label">Command that will run</div>
            <div className="public-key-box user-select-auto">{request.command}</div>
          </>
        )}

        {!blocked && needsPassword && (
          <div className="mt-3">
            {canReuse && (
              <Form.Check
                type="checkbox"
                id="elevation-reuse-login"
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
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && canGrant) grant();
                  }}
                  autoComplete="off"
                  autoFocus
                />
              </Form.Group>
            )}

            <Form.Text className="text-body-secondary">
              Sent to <code>sudo -S</code> over the existing encrypted channel,
              never as part of the command line — so it stays out of the remote
              process list, and it is not kept after this run.
            </Form.Text>
          </div>
        )}
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={cancel}>
          Cancel
        </Button>
        {request.unprivilegedLabel && (
          <Button
            variant="outline-primary"
            onClick={() => onSettle({ outcome: "unprivileged" })}
          >
            {request.unprivilegedLabel}
          </Button>
        )}
        <Button
          variant={request.destructive ? "danger" : "primary"}
          onClick={grant}
          disabled={!canGrant}
        >
          Elevate and run
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
