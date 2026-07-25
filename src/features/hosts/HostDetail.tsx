import { useEffect, useState } from "react";
import { Alert, Button } from "react-bootstrap";
import { ChevronLeft, Pencil, Plug, Power, Trash2, Unplug } from "lucide-react";
import { useHosts } from "./HostsProvider";
import { HostFeatureNav, HOST_FEATURES, type HostFeature } from "./HostFeatureNav";
import { StatusBadge } from "./StatusIndicator";
import { TerminalTabs } from "./TerminalTabs";
import { OsBadge, OverviewPane } from "./panes/OverviewPane";
import { PlannedPane } from "./panes/PlannedPane";
import { useHostActions } from "./useHostActions";
import type { Navigate } from "../../navigation";

export function HostDetail({
  hostId,
  onNavigate,
}: {
  hostId: string;
  onNavigate: Navigate;
}) {
  const { getHost, getConnection, getHealth } = useHosts();
  const [feature, setFeature] = useState<HostFeature>("overview");

  const host = getHost(hostId);
  const connection = getConnection(hostId);
  const connected = Boolean(connection);

  const { actions, dialogs } = useHostActions({
    onOpenTerminal: () => setFeature("terminal"),
  });

  // Landing on a different host starts at Overview rather than carrying the
  // previous host's tab across — Services on one box says nothing about another.
  useEffect(() => setFeature("overview"), [hostId]);

  // Every tab except Overview needs a live session. Losing one mid-view would
  // otherwise leave you staring at a pane that cannot refresh.
  useEffect(() => {
    if (connected) return;
    const definition = HOST_FEATURES.find((entry) => entry.id === feature);
    if (definition?.needsSession) setFeature("overview");
  }, [connected, feature]);

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

  const selected = HOST_FEATURES.find((entry) => entry.id === feature);
  const locked = Boolean(selected?.needsSession) && !connected;

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

      <header className="d-flex flex-wrap align-items-center gap-3 mb-3">
        <div className="me-auto">
          <div className="d-flex align-items-center gap-2 mb-1">
            <h1 className="page-title">{host.label}</h1>
            <StatusBadge status={host.status} />
            <OsBadge connection={connection} />
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

      <HostFeatureNav active={feature} connected={connected} onSelect={setFeature} />

      <div className="feature-pane">
        {locked ? (
          <Alert variant="secondary" className="mb-0">
            Connect to this host to use {selected?.label}. It reads from the live
            session — there is nothing to show without one.
          </Alert>
        ) : feature === "overview" ? (
          <OverviewPane
            host={host}
            connection={connection}
            health={getHealth(hostId)}
          />
        ) : feature === "terminal" ? (
          <TerminalTabs hostId={hostId} />
        ) : (
          <PlannedPane feature={feature} os={connection?.os} />
        )}
      </div>

      {dialogs}
    </div>
  );
}
