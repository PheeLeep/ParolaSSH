import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import {
  ChevronDown,
  ChevronRight,
  Maximize2,
  Minimize2,
  ShieldAlert,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import * as api from "./api";
import { errorMessage } from "./api";
import { useHosts } from "./HostsProvider";
import { ELEVATION_LABELS } from "./types";

/**
 * One prompt for every privileged action, asked at the moment of elevating.
 *
 * The prompt always appears - what is worth confirming is that a command runs
 * as root on someone else's machine, not whether a password happens to be
 * involved - and adapts to the host's elevation route:
 *
 *  - `sudoPassword` - asks for the account password, offering the one this
 *    session logged in with rather than making it be typed twice.
 *  - `notNeeded` / `sudoNoPassword` / `windowsAdminToken` - a consent step
 *    showing the literal command.
 *  - `unavailable` - explains why there is no route to root.
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
  /** Set when the action is still useful unelevated (the audit's read-only
   *  checks, say). Becomes a second button labelled with this text. */
  unprivilegedLabel?: string;
}

export type ElevationGrant =
  /** Null when sudo needs no password, or when the session's own login
   *  password should be used - that one never travels through the webview. */
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
    // A second request while one is open would strand the first promise.
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
  const { getConnection, getHost } = useHosts();
  const connection = request ? getConnection(request.hostId) : undefined;
  const host = request ? getHost(request.hostId) : undefined;

  const [password, setPassword] = useState("");
  /** Reuse the password this session logged in with, rather than retyping. */
  const [reuseLogin, setReuseLogin] = useState(true);
  const [commandExpanded, setCommandExpanded] = useState(false);
  const [showDetails, setShowDetails] = useState(false);
  /** A sudo password typed earlier this session and accepted by sudo. */
  const [keptSudo, setKeptSudo] = useState(false);
  /** Checking a typed password with sudo; the prompt is locked meanwhile. */
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The request a verification started for, so a stale answer is dropped. */
  const requestRef = useRef(request);
  requestRef.current = request;

  useEffect(() => {
    setPassword("");
    setBusy(false);
    setError(null);
    setReuseLogin(true);
    setCommandExpanded(false);
    setShowDetails(false);
    setKeptSudo(false);
    if (!request) return;

    let cancelled = false;
    api
      .hasKeptSudoPassword(request.hostId)
      .then((kept) => {
        if (!cancelled) setKeptSudo(kept);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [request]);

  if (!request) return null;

  const cancel = () => {
    if (!busy) onSettle({ outcome: "cancelled" });
  };

  // Raised from inside other modals, which Bootstrap's default stacking would
  // otherwise render on top.
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
  const canReuse = needsPassword && (keptSudo || connection.hasLoginPassword);
  const usingLogin = canReuse && reuseLogin;
  const canGrant =
    !busy && !blocked && (!needsPassword || usingLogin || password.length > 0);

  const grant = async () => {
    if (!canGrant) return;
    // Null tells Rust to use the kept sudo password, else the login one.
    // Neither travels through the webview.
    if (!needsPassword || usingLogin) {
      onSettle({ outcome: "granted", password: null });
      return;
    }
    const asked = request;
    setBusy(true);
    setError(null);
    try {
      // Accepted means kept on the session, so the caller can pass null.
      await api.verifySudoPassword(asked.hostId, password);
      if (requestRef.current !== asked) return;
      setPassword("");
      onSettle({ outcome: "granted", password: null });
    } catch (caught) {
      if (requestRef.current !== asked) return;
      setError(errorMessage(caught));
      setBusy(false);
    }
  };

  const title = blocked
    ? "Cannot elevate"
    : elevation.kind === "windowsAdminToken"
      ? "Allow administrator access?"
      : "Allow root access?";

  return (
    <Modal show onHide={cancel} centered backdrop="static" {...stacked}>
      <Modal.Header closeButton={!busy} className="py-2">
        <Modal.Title className="h6 d-flex align-items-center gap-2 mb-0">
          {blocked ? (
            <ShieldAlert className="icon-sm" aria-hidden="true" />
          ) : (
            <ShieldCheck className="icon-sm" aria-hidden="true" />
          )}
          {title}
        </Modal.Title>
      </Modal.Header>

      <Modal.Body className="d-flex flex-column gap-3">
        {/* UAC-style: what runs and where, before anything else. */}
        <div>
          <div className="fs-5 fw-semibold lh-sm">{request.summary}</div>
          <div className="small text-body-secondary mt-1">
            <span className="font-monospace">
              {user}@{host?.label ?? host?.hostname ?? request.hostId}
            </span>
            {" · "}
            {ELEVATION_LABELS[elevation.kind]}
          </div>
        </div>

        {error && (
          <Alert variant="danger" className="small py-2 mb-0 text-prewrap">
            {error}
          </Alert>
        )}

        {(blocked || request.destructive) && (
          <Alert
            variant={blocked ? "danger" : "warning"}
            className="d-flex gap-2 small py-2 mb-0"
          >
            <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>
              {elevation.kind === "unavailable"
                ? elevation.reason
                : "This interrupts or changes the machine."}
            </div>
          </Alert>
        )}

        {request.command && (
          <div>
            <div className="d-flex align-items-center mb-1">
              <span className="detail-grid__label mb-0 me-auto">Command</span>
              <Button
                size="sm"
                variant="link"
                className="p-0 text-decoration-none text-body-secondary small"
                onClick={() => setCommandExpanded((value) => !value)}
                aria-expanded={commandExpanded}
              >
                {commandExpanded ? (
                  <Minimize2 className="icon-sm" aria-hidden="true" />
                ) : (
                  <Maximize2 className="icon-sm" aria-hidden="true" />
                )}
                {commandExpanded ? "Collapse" : "Expand"}
              </Button>
            </div>
            {commandExpanded ? (
              <div className="public-key-box elevation-command is-expanded user-select-auto">
                <span className="elevation-command__text">{request.command}</span>
              </div>
            ) : (
              // Collapsed to two lines; a click opens the full command.
              <div
                role="button"
                tabIndex={0}
                className="public-key-box elevation-command"
                title="Show the full command"
                onClick={() => setCommandExpanded(true)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    setCommandExpanded(true);
                  }
                }}
              >
                <span className="elevation-command__text">{request.command}</span>
              </div>
            )}
          </div>
        )}

        {!blocked && needsPassword && (
          <div>
            {usingLogin && (
              <div className="d-flex align-items-center gap-2 small">
                <span className="text-body-secondary">
                  {keptSudo
                    ? "Using the sudo password entered earlier this session."
                    : `Using the password you logged in with as ${user}.`}
                </span>
                <Button
                  size="sm"
                  variant="link"
                  className="p-0 ms-auto text-decoration-none small"
                  disabled={busy}
                  onClick={() => {
                    if (keptSudo) {
                      void api.forgetSudoPassword(request.hostId).catch(() => undefined);
                      setKeptSudo(false);
                      // The login password, if any, is still offered.
                      if (!connection.hasLoginPassword) setReuseLogin(false);
                    } else {
                      setReuseLogin(false);
                    }
                  }}
                >
                  {keptSudo ? "Forget it" : "Use a different password"}
                </Button>
              </div>
            )}

            {!usingLogin && (
              <Form.Control
                type="password"
                placeholder={`sudo password for ${user}`}
                aria-label={`sudo password for ${user}`}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void grant();
                }}
                disabled={busy}
                autoComplete="off"
                autoFocus
              />
            )}
          </div>
        )}

        {!blocked && (
          <div>
            <Button
              size="sm"
              variant="link"
              className="p-0 text-decoration-none text-body-secondary small"
              onClick={() => setShowDetails((value) => !value)}
              aria-expanded={showDetails}
            >
              {showDetails ? (
                <ChevronDown className="icon-sm" aria-hidden="true" />
              ) : (
                <ChevronRight className="icon-sm" aria-hidden="true" />
              )}
              {showDetails ? "Hide details" : "Show details"}
            </Button>
            {showDetails && (
              <p className="small text-body-secondary mt-2 mb-0">
                {/* The explanation already says how the password travels; only
                    how long it lives is added here. */}
                <WithInlineCode text={elevationExplanation} />
                {needsPassword &&
                  " A typed password is checked with sudo first; once accepted it is kept in memory for this session, so later prompts can reuse it, and wiped on disconnect."}
              </p>
            )}
          </div>
        )}
      </Modal.Body>

      <Modal.Footer className="py-2">
        <Button variant="outline-secondary" onClick={cancel} disabled={busy}>
          {blocked ? "Close" : "Cancel"}
        </Button>
        {!blocked && request.unprivilegedLabel && (
          <Button
            variant="outline-primary"
            onClick={() => onSettle({ outcome: "unprivileged" })}
            disabled={busy}
          >
            {request.unprivilegedLabel}
          </Button>
        )}
        {!blocked && (
          <Button
            variant={request.destructive ? "danger" : "primary"}
            onClick={() => void grant()}
            disabled={!canGrant}
          >
            {busy && (
              <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
            )}
            {busy ? "Checking password…" : "Elevate and run"}
          </Button>
        )}
      </Modal.Footer>
    </Modal>
  );
}

/** Renders `backticked` spans from host-side messages as code. */
function WithInlineCode({ text }: { text: string }) {
  return (
    <>
      {text.split("`").map((part, index) =>
        index % 2 === 1 ? <code key={index}>{part}</code> : part,
      )}
    </>
  );
}
