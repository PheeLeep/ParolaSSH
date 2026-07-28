import { useEffect, useState } from "react";
import { Alert, Button } from "react-bootstrap";
import { ChevronLeft, Pencil, Plug, Power, Trash2, Unplug } from "lucide-react";
import { useHosts } from "./HostsProvider";
import { HostFeatureNav, HOST_FEATURES, type HostFeature } from "./HostFeatureNav";
import { StatusBadge } from "./StatusIndicator";
import { TerminalTabs } from "./TerminalTabs";
import { AuditPane } from "./panes/AuditPane";
import { OsBadge, OverviewPane } from "./panes/OverviewPane";
import { PerformancePane } from "./panes/PerformancePane";
import { ServicesPane } from "./panes/ServicesPane";
import { TasksPane } from "./panes/TasksPane";
import { UpdatesPane } from "./panes/UpdatesPane";
import { useHostActions } from "./useHostActions";
import { FilesPane } from "../transfers/FilesPane";
import { VpnGlyph } from "../vpn/VpnGlyph";
import { PaneBoundary } from "../../components/PaneBoundary";
import type { Navigate } from "../../navigation";

export function HostDetail({
  hostId,
  feature: requested,
  shellId,
  onNavigate,
}: {
  hostId: string;
  /** Set when the caller knows which tab it wants — Sessions links to one. */
  feature?: HostFeature;
  shellId?: number;
  onNavigate: Navigate;
}) {
  const { getHost, getConnection, getHealth } = useHosts();
  const [feature, setFeature] = useState<HostFeature>(requested ?? "overview");

  const host = getHost(hostId);
  const connection = getConnection(hostId);
  const connected = Boolean(connection);

  const { actions, dialogs } = useHostActions({
    onOpenTerminal: () => setFeature("terminal"),
  });

  // Landing on a different host starts at Overview rather than carrying the
  // previous host's tab across — Services on one box says nothing about
  // another — unless the link asked for a tab by name.
  useEffect(() => setFeature(requested ?? "overview"), [hostId, requested]);

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
  // The terminal and the file browser want the whole window; the other panes
  // read better at their natural height.
  const fill = (feature === "terminal" || feature === "files") && !locked;

  return (
    <div className={`page${fill ? " page--fill" : ""}`}>
      <Button
        variant="link"
        size="sm"
        className="page-back p-0 mb-3 text-decoration-none text-body-secondary"
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
          <p className="text-body-secondary font-monospace small mb-0 d-inline-flex align-items-center gap-1">
            {host.username}@{host.hostname}:{host.port}
            <VpnGlyph hostname={host.hostname} />
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

      <div className={`feature-pane${fill ? " feature-pane--fill" : ""}`}>
        <PaneBoundary resetKey={`${hostId}:${feature}`}>
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
            <TerminalTabs hostId={hostId} focusShellId={shellId} />
          ) : feature === "services" ? (
            <ServicesPane hostId={hostId} />
          ) : feature === "performance" ? (
            <PerformancePane hostId={hostId} />
          ) : feature === "tasks" ? (
            <TasksPane hostId={hostId} />
          ) : feature === "updates" ? (
            <UpdatesPane hostId={hostId} />
          ) : feature === "audit" ? (
            <AuditPane hostId={hostId} />
          ) : (
            <FilesPane hostId={hostId} />
          )}
        </PaneBoundary>
      </div>

      {dialogs}
    </div>
  );
}
