import { Button, Card } from "react-bootstrap";
import { ChevronLeft, Pencil, Plug, Trash2 } from "lucide-react";
import { useHosts } from "./HostsProvider";
import { StatusBadge } from "./StatusIndicator";
import { AUTH_METHOD_LABELS } from "./types";
import { formatAbsolute, formatRelative } from "../../lib/format";
import type { Navigate } from "../../navigation";

export function HostDetail({
  hostId,
  onNavigate,
}: {
  hostId: string;
  onNavigate: Navigate;
}) {
  const { getHost, connect, edit, remove } = useHosts();
  const host = getHost(hostId);

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

        <div className="d-flex gap-2">
          <Button variant="primary" onClick={() => connect(host)}>
            <Plug aria-hidden="true" />
            Connect
          </Button>
          <Button variant="outline-secondary" onClick={() => edit(host)}>
            <Pencil aria-hidden="true" />
            Edit
          </Button>
          <Button variant="outline-secondary" onClick={() => remove(host)}>
            <Trash2 aria-hidden="true" />
            <span className="visually-hidden">Delete {host.label}</span>
          </Button>
        </div>
      </header>

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
              <dd>{AUTH_METHOD_LABELS[host.authMethod]}</dd>
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
          </dl>
        </Card.Body>
      </Card>
    </div>
  );
}
