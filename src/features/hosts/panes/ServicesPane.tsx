import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Form, Modal, Spinner, Table } from "react-bootstrap";
import {
  CircleCheck,
  CircleX,
  Play,
  RefreshCw,
  RotateCcw,
  Square,
} from "lucide-react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "../api";
import { errorMessage } from "../api";
import { useElevation } from "../ElevationProvider";
import { useHosts } from "../HostsProvider";
import {
  SERVICE_STATE_LABELS,
  type ServiceAction,
  type ServiceActionRequest,
  type ServiceEntry,
  type ServiceLog,
  type ServiceOutcome,
  type ServicePlan,
} from "../types";

/** How much followed output the pane keeps. Old lines fall off the top. */
const FOLLOW_BUFFER_LIMIT = 100_000;

const STATE_BADGE: Record<ServiceEntry["state"], string> = {
  running: "status-badge status-badge--connected",
  stopped: "status-badge",
  failed: "status-badge status-badge--offline",
  other: "status-badge status-badge--reachable",
};

export function ServicesPane({ hostId }: { hostId: string }) {
  const { getConnection } = useHosts();
  const connection = getConnection(hostId);

  const [services, setServices] = useState<ServiceEntry[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<ServiceEntry | null>(null);
  const [pending, setPending] = useState<ServiceActionRequest | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setServices(await api.listServices(hostId));
    } catch (caught) {
      setServices(null);
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const visible = (services ?? []).filter((service) => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return true;
    return (
      service.name.toLowerCase().includes(needle) ||
      service.description.toLowerCase().includes(needle)
    );
  });

  return (
    <div className="d-flex flex-column gap-3">
      <div className="d-flex flex-wrap align-items-center gap-2">
        <Form.Control
          type="search"
          className="w-auto flex-grow-1"
          style={{ maxWidth: "24rem" }}
          placeholder="Filter services…"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        <span className="text-body-secondary small me-auto">
          {services ? `${visible.length} of ${services.length}` : ""}
        </span>
        <Button
          size="sm"
          variant="outline-secondary"
          disabled={loading}
          onClick={() => void refresh()}
        >
          <RefreshCw className="icon-sm" aria-hidden="true" />
          Refresh
        </Button>
      </div>

      {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

      {loading && !services ? (
        <div className="d-flex align-items-center gap-2 text-body-secondary py-4">
          <Spinner animation="border" size="sm" aria-hidden="true" />
          Reading the service list…
        </div>
      ) : services && (
        <div className="services-table">
          <Table hover size="sm" className="align-middle mb-0">
            <thead>
              <tr>
                <th style={{ width: "8rem" }}>State</th>
                <th>Service</th>
                <th className="d-none d-lg-table-cell">Description</th>
                <th style={{ width: "10rem" }} />
              </tr>
            </thead>
            <tbody>
              {visible.map((service) => (
                <tr
                  key={service.name}
                  role="button"
                  className={selected?.name === service.name ? "table-active" : undefined}
                  onClick={() => setSelected(service)}
                >
                  <td>
                    <span className={STATE_BADGE[service.state]} title={service.detail}>
                      {SERVICE_STATE_LABELS[service.state]}
                    </span>
                  </td>
                  <td className="font-monospace small">{service.name}</td>
                  <td className="d-none d-lg-table-cell text-body-secondary small">
                    {service.description}
                  </td>
                  <td className="text-end">
                    <div className="d-inline-flex gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        size="sm"
                        variant="outline-secondary"
                        title={`Start ${service.name}`}
                        disabled={service.state === "running"}
                        onClick={() => setPending({ action: "start", unit: service.name })}
                      >
                        <Play className="icon-sm" aria-hidden="true" />
                      </Button>
                      <Button
                        size="sm"
                        variant="outline-secondary"
                        title={`Stop ${service.name}`}
                        disabled={service.state === "stopped"}
                        onClick={() => setPending({ action: "stop", unit: service.name })}
                      >
                        <Square className="icon-sm" aria-hidden="true" />
                      </Button>
                      <Button
                        size="sm"
                        variant="outline-secondary"
                        title={`Restart ${service.name}`}
                        onClick={() => setPending({ action: "restart", unit: service.name })}
                      >
                        <RotateCcw className="icon-sm" aria-hidden="true" />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
              {visible.length === 0 && (
                <tr>
                  <td colSpan={4} className="text-body-secondary py-3">
                    No service matches “{filter}”.
                  </td>
                </tr>
              )}
            </tbody>
          </Table>
        </div>
      )}

      {selected && (
        <ServiceHistory
          hostId={hostId}
          service={selected}
          canFollow={connection?.os === "linux"}
        />
      )}

      <ServiceActionDialog
        hostId={hostId}
        request={pending}
        onClose={() => setPending(null)}
        onDone={() => void refresh()}
      />
    </div>
  );
}

/** The selected service's history: last journal lines or SCM events, with a
 *  follow toggle where the platform supports one. */
function ServiceHistory({
  hostId,
  service,
  canFollow,
}: {
  hostId: string;
  service: ServiceEntry;
  canFollow: boolean;
}) {
  const [log, setLog] = useState<ServiceLog | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [follow, setFollow] = useState(false);
  const [followText, setFollowText] = useState("");

  // Leaving the service or the pane always stops following: a follow holds a
  // remote process and a stream slot, so nothing keeps it alive off screen.
  useEffect(() => {
    setFollow(false);
    setFollowText("");
  }, [service.name, hostId]);

  useEffect(() => {
    if (follow) return; // the stream includes the tail; no one-shot needed
    let cancelled = false;
    setLoading(true);
    setLogError(null);
    void (async () => {
      try {
        const result = await api.serviceLog(hostId, service.name, service.description);
        if (!cancelled) setLog(result);
      } catch (caught) {
        if (!cancelled) {
          setLog(null);
          setLogError(errorMessage(caught));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [hostId, service.name, service.description, follow]);

  // The follow lifecycle. The listener registers before the stream opens and
  // filters on the id it later learns, so the opening batch is not missed;
  // cleanup unlistens *and* closes the stream, or the remote `journalctl -f`
  // would keep running against the stream cap.
  const streamIdRef = useRef<number | null>(null);
  useEffect(() => {
    if (!follow) return;
    let cancelled = false;
    let opened: number | null = null;
    const unlisteners: UnlistenFn[] = [];
    setFollowText("");

    void (async () => {
      try {
        unlisteners.push(
          await api.onStreamOutput(
            hostId,
            (id) => id === streamIdRef.current,
            (event) => {
              setFollowText((previous) =>
                (previous + event.chunk).slice(-FOLLOW_BUFFER_LIMIT),
              );
            },
          ),
        );
        unlisteners.push(
          await api.onStreamClosed(
            hostId,
            (id) => id === streamIdRef.current,
            () => setFollow(false),
          ),
        );

        const id = await api.followServiceLog(hostId, service.name);
        if (cancelled) {
          void api.closeStream(hostId, id);
          return;
        }
        streamIdRef.current = id;
        opened = id;
      } catch (caught) {
        setLogError(errorMessage(caught));
        setFollow(false);
      }
    })();

    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
      streamIdRef.current = null;
      if (opened !== null) void api.closeStream(hostId, opened);
    };
  }, [follow, hostId, service.name]);

  return (
    <div>
      <div className="d-flex flex-wrap align-items-center gap-2 mb-2">
        <h2 className="h6 mb-0 font-monospace">{service.name}</h2>
        <span className="text-body-secondary small me-auto">
          {canFollow ? "journal" : "Service Control Manager events"}
        </span>
        {canFollow && (
          <Form.Check
            type="switch"
            id="service-log-follow"
            label="Follow"
            checked={follow}
            onChange={(event) => setFollow(event.target.checked)}
          />
        )}
      </div>

      {logError && <Alert variant="danger" className="text-prewrap">{logError}</Alert>}
      {log?.note && !follow && (
        <Alert variant="secondary" className="small py-2">{log.note}</Alert>
      )}

      {loading && !follow ? (
        <div className="d-flex align-items-center gap-2 text-body-secondary py-3">
          <Spinner animation="border" size="sm" aria-hidden="true" />
          Reading history…
        </div>
      ) : (
        <pre className="command-output service-log user-select-auto mb-0">
          {follow
            ? followText || "Waiting for output…"
            : log && log.lines.length > 0
              ? log.lines.join("\n")
              : "No recent entries for this service."}
        </pre>
      )}
    </div>
  );
}

/** Confirmation for start/stop/restart, showing the literal command — the
 *  same contract as the Power dialog: no mystery buttons. */
function ServiceActionDialog({
  hostId,
  request,
  onClose,
  onDone,
}: {
  hostId: string;
  request: ServiceActionRequest | null;
  onClose: () => void;
  onDone: () => void;
}) {
  const requestElevation = useElevation();

  const [plan, setPlan] = useState<ServicePlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ServiceOutcome | null>(null);

  useEffect(() => {
    setPlan(null);
    setPlanError(null);
    setError(null);
    setOutcome(null);
    setBusy(false);
    if (!request) return;

    let cancelled = false;
    void (async () => {
      try {
        const preview = await api.previewServiceAction(hostId, request);
        if (!cancelled) setPlan(preview);
      } catch (caught) {
        if (!cancelled) setPlanError(errorMessage(caught));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [hostId, request]);

  if (!request) return null;

  const canRun = !busy && plan !== null;

  const run = async () => {
    // Starting or stopping a unit is a privileged act even where sudo is
    // configured to want no password, so the prompt is not conditional on the
    // plan needing one — the shared prompt decides what to ask for.
    const grant = await requestElevation({
      hostId,
      summary: `${verb[request.action]} ${request.unit}`,
      command: plan?.command ?? null,
      destructive: request.action !== "start",
    });
    if (grant.outcome !== "granted") return;

    setBusy(true);
    setError(null);
    try {
      setOutcome(await api.serviceAction(hostId, request, grant.password));
      onDone();
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const verb: Record<ServiceAction, string> = {
    start: "Start",
    stop: "Stop",
    restart: "Restart",
  };

  return (
    <Modal show onHide={onClose} centered backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title>
          {verb[request.action]} {request.unit}
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
              <div className="text-prewrap">{outcome.message}</div>
            </Alert>
            <div className="detail-grid__label">Command that ran</div>
            <div className="public-key-box user-select-auto">{outcome.command}</div>
          </Modal.Body>
          <Modal.Footer>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </Modal.Footer>
        </>
      ) : (
        <>
          <Modal.Body>
            {error && <Alert variant="danger" className="text-prewrap">{error}</Alert>}
            {planError && <Alert variant="warning" className="text-prewrap mb-0">{planError}</Alert>}

            {plan && (
              <>
                <div className="detail-grid__label">Command that will run</div>
                <div className="public-key-box user-select-auto">{plan.command}</div>
              </>
            )}

          </Modal.Body>
          <Modal.Footer>
            <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant={request.action === "start" ? "primary" : "danger"}
              onClick={run}
              disabled={!canRun}
            >
              {busy && (
                <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
              )}
              {busy ? "Running…" : plan?.summary ?? verb[request.action]}
            </Button>
          </Modal.Footer>
        </>
      )}
    </Modal>
  );
}
