import { useEffect, useState } from "react";
import { Badge, Button, Card } from "react-bootstrap";
import {
  ChevronLeft,
  Pencil,
  Plug,
  Power,
  ShieldCheck,
  SquareTerminal,
  Trash2,
  Unplug,
} from "lucide-react";
import { useHosts } from "./HostsProvider";
import { StatusBadge } from "./StatusIndicator";
import { TerminalPane } from "./TerminalPane";
import { useHostActions } from "./useHostActions";
import { AUTH_METHOD_LABELS, ELEVATION_LABELS, OS_LABELS } from "./types";
import { formatAbsolute, formatRelative } from "../../lib/format";
import type { Navigate } from "../../navigation";

export function HostDetail({
  hostId,
  onNavigate,
}: {
  hostId: string;
  onNavigate: Navigate;
}) {
  const { getHost, getConnection } = useHosts();
  const [showTerminal, setShowTerminal] = useState(false);

  const host = getHost(hostId);
  const connection = getConnection(hostId);

  const { actions, dialogs } = useHostActions({
    onOpenTerminal: () => setShowTerminal(true),
  });

  // Losing the session closes the pane: an xterm attached to nothing just
  // swallows keystrokes, which looks like a hang.
  useEffect(() => {
    if (!connection) setShowTerminal(false);
  }, [connection]);

  // Switching hosts must not carry the previous host's terminal across.
  useEffect(() => setShowTerminal(false), [hostId]);

  if (!host) {
    return (
      <div className="page">
        <p className="text-body-secondary">
          That host no longer exists.{" "}
          <Button
            variant="link"
            className="p-0 align-baseline"
            onClick={() => onNavigate({ kind: "hosts" })}
          >
            Back to all hosts
          </Button>
        </p>
      </div>
    );
  }

  const connected = Boolean(connection);

  return (
    <div className="page">
      <Button
        variant="link"
        size="sm"
        className="p-0 mb-3 text-decoration-none text-body-secondary"
        onClick={() => onNavigate({ kind: "hosts" })}
      >
        <ChevronLeft className="icon-sm" aria-hidden="true" />
        All hosts
      </Button>

      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <div className="d-flex align-items-center gap-2 mb-1">
            <h1 className="page-title">{host.label}</h1>
            <StatusBadge status={host.status} />
          </div>
          <p className="text-body-secondary font-monospace small mb-0">
            {host.username}@{host.hostname}:{host.port}
          </p>
        </div>

        <div className="d-flex flex-wrap gap-2">
          {connected ? (
            <Button variant="outline-secondary" onClick={() => actions.onDisconnect(host)}>
              <Unplug aria-hidden="true" />
              Disconnect
            </Button>
          ) : (
            <Button variant="primary" onClick={() => actions.onConnect(host)}>
              <Plug aria-hidden="true" />
              Connect
            </Button>
          )}

          <Button
            variant="outline-secondary"
            disabled={!connected}
            title={connected ? undefined : "Connect first"}
            onClick={() => setShowTerminal(true)}
          >
            <SquareTerminal aria-hidden="true" />
            Terminal
          </Button>

          <Button
            variant="outline-secondary"
            disabled={!connected}
            title={connected ? undefined : "Connect first"}
            onClick={() => actions.onPower(host)}
          >
            <Power aria-hidden="true" />
            Power
          </Button>

          <Button variant="outline-secondary" onClick={() => actions.onEdit(host)}>
            <Pencil aria-hidden="true" />
            Edit
          </Button>
          <Button variant="outline-secondary" onClick={() => actions.onDelete(host)}>
            <Trash2 aria-hidden="true" />
            <span className="visually-hidden">Delete {host.label}</span>
          </Button>
        </div>
      </header>

      {/* Only meaningful while connected — everything here was learned from
          the live session, not from the saved record. */}
      {connection && (
        <Card className="mb-3">
          <Card.Body>
            <div className="d-flex flex-wrap align-items-center gap-2 mb-3">
              <Badge bg="secondary">{OS_LABELS[connection.os]}</Badge>
              <span className="text-body-secondary small font-monospace">
                {connection.osDetail}
              </span>
            </div>

            <div className="d-flex gap-2">
              <ShieldCheck
                className={`icon-sm flex-shrink-0 mt-1 ${
                  connection.elevation.kind === "unavailable"
                    ? "text-danger"
                    : "text-success"
                }`}
                aria-hidden="true"
              />
              <div>
                <div className="fw-semibold">
                  {ELEVATION_LABELS[connection.elevation.kind]}
                </div>
                <div className="text-body-secondary small">
                  {connection.elevationExplanation}
                </div>
              </div>
            </div>

            {connection.fingerprint && (
              <div className="mt-3">
                <div className="detail-grid__label">Host key</div>
                <code className="small text-break user-select-auto">
                  {connection.fingerprint}
                </code>
              </div>
            )}
          </Card.Body>
        </Card>
      )}

      {showTerminal && connected && (
        <div className="mb-3">
          <TerminalPane hostId={host.id} onClose={() => setShowTerminal(false)} />
        </div>
      )}

      <Card>
        <Card.Body>
          <dl className="detail-grid mb-0">
            <div>
              <dt>Hostname</dt>
              <dd className="font-monospace">{host.hostname}</dd>
            </div>
            <div>
              <dt>Port</dt>
              <dd className="font-monospace">{host.port}</dd>
            </div>
            <div>
              <dt>Username</dt>
              <dd>{host.username}</dd>
            </div>
            <div>
              <dt>Authentication</dt>
              <dd>
                {AUTH_METHOD_LABELS[host.authMethod]}
                {host.keyPath && (
                  <div className="text-body-secondary small font-monospace">
                    {host.keyPath}
                  </div>
                )}
              </dd>
            </div>
            <div>
              <dt>Group</dt>
              <dd>{host.group}</dd>
            </div>
            <div>
              <dt>Last connected</dt>
              <dd title={formatAbsolute(host.lastConnected)}>
                {formatRelative(host.lastConnected)}
              </dd>
            </div>
            <div>
              <dt>Tags</dt>
              <dd className="d-flex flex-wrap gap-1">
                {host.tags.length === 0 ? (
                  <span className="text-body-secondary">None</span>
                ) : (
                  host.tags.map((tag) => (
                    <span key={tag} className="tag-chip">
                      {tag}
                    </span>
                  ))
                )}
              </dd>
            </div>
            {host.notes && (
              <div>
                <dt>Notes</dt>
                <dd className="text-prewrap">{host.notes}</dd>
              </div>
            )}
          </dl>
        </Card.Body>
      </Card>

      {dialogs}
    </div>
  );
}
