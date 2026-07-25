import { useEffect, useState } from "react";
import { Alert, Button, Card, ListGroup, Table } from "react-bootstrap";
import { ChevronRight, RefreshCw, Shield, TriangleAlert } from "lucide-react";
import { formatRelative } from "../../lib/format";
import { useHosts, type HostRow } from "../hosts/HostsProvider";
import { StatusDot } from "../hosts/StatusIndicator";
import {
  conflictNote,
  VPN_LABELS,
  type VpnBinding,
  type VpnKind,
  type VpnResource,
  type VpnStatus,
} from "./types";
import { useVpn } from "./VpnProvider";
import type { Navigate } from "../../navigation";

/** Overview first, then one tab per installed client — never a tab for an
 *  absent client: a dead "not installed" tab would be pure noise. */
type VpnTab = "overview" | VpnKind;

type BoundHost = { host: HostRow; binding: VpnBinding };

/**
 * Everything the app knows about the VPNs on this machine, one client per
 * tab in the same strip the host detail page uses.
 *
 * Strictly an observer — there is no connect button here, and that is a
 * trust decision, not a gap: starting a VPN means elevating privileges and
 * changing routes machine-wide, which belongs to the VPN's own UI.
 */
export function VpnPage({ onNavigate }: { onNavigate: Navigate }) {
  const { statuses, resources, bindingFor, lastChecked, refresh } = useVpn();
  const { hosts } = useHosts();
  const [refreshing, setRefreshing] = useState(false);
  const [tab, setTab] = useState<VpnTab>("overview");

  const installed = statuses.filter((status) => status.installed);
  const up = installed.filter((status) => status.up);

  // A client can disappear between polls (uninstalled, or detection lost
  // it); its tab must not linger as a blank pane.
  const installedKey = installed.map((status) => status.kind).join(",");
  useEffect(() => {
    if (tab !== "overview" && !installedKey.split(",").includes(tab)) {
      setTab("overview");
    }
  }, [tab, installedKey]);

  const bound: BoundHost[] = hosts.flatMap((host) => {
    const binding = bindingFor(host.hostname);
    return binding ? [{ host, binding }] : [];
  });

  /** A client's hosts, ambiguous bindings included on every client's tab. */
  const boundVia = (kind: VpnKind) =>
    bound.filter(({ binding }) => binding.kind === kind || binding.kind === null);

  const onRefresh = async () => {
    setRefreshing(true);
    try {
      await refresh();
    } finally {
      setRefreshing(false);
    }
  };

  const selected = installed.find((status) => status.kind === tab);

  return (
    <div className="page">
      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">VPN</h1>
          <p className="text-body-secondary mb-0">
            {installed.length === 0
              ? "No VPN clients detected"
              : `${installed.length} ${installed.length === 1 ? "client" : "clients"} installed · ${up.length} connected`}
            {lastChecked && <> · checked {formatRelative(lastChecked)}</>}
          </p>
        </div>

        <Button
          variant="outline-secondary"
          onClick={() => void onRefresh()}
          disabled={refreshing}
        >
          <RefreshCw
            className={refreshing ? "icon-spin" : undefined}
            aria-hidden="true"
          />
          {refreshing ? "Checking…" : "Refresh"}
        </Button>
      </header>

      {/* Above the tabs because it concerns both clients, not one. */}
      {up.length >= 2 && (
        <Alert variant="warning" className="d-flex align-items-center gap-2">
          <TriangleAlert className="flex-shrink-0" aria-hidden="true" />
          <span>{conflictNote(up.map((status) => VPN_LABELS[status.kind]))}</span>
        </Alert>
      )}

      {installed.length === 0 ? (
        <Card body className="text-body-secondary mb-4">
          ParolaSSH looks for Tailscale and Twingate on this machine and found
          neither. If a saved host lives behind one of them, install and
          connect that client — connections here will then flow through it
          automatically.
        </Card>
      ) : (
        <>
          <nav className="feature-nav" aria-label="VPN sections">
            <button
              type="button"
              className={`feature-nav__item${tab === "overview" ? " is-active" : ""}`}
              onClick={() => setTab("overview")}
              aria-current={tab === "overview" ? "page" : undefined}
            >
              <Shield className="icon-sm" aria-hidden="true" />
              Overview
            </button>

            {installed.map((status) => (
              <button
                key={status.kind}
                type="button"
                className={`feature-nav__item${tab === status.kind ? " is-active" : ""}`}
                onClick={() => setTab(status.kind)}
                aria-current={tab === status.kind ? "page" : undefined}
                title={`${VPN_LABELS[status.kind]} — ${status.detail}`}
              >
                <span
                  className={`status-dot status-dot--${status.up ? "connected" : "offline"}`}
                  aria-hidden="true"
                />
                {VPN_LABELS[status.kind]}
              </button>
            ))}
          </nav>

          {tab === "overview" ? (
            <OverviewPane
              installed={installed}
              resources={resources}
              bound={bound}
              onSelect={setTab}
              onNavigate={onNavigate}
            />
          ) : (
            selected && (
              <ClientPane
                status={selected}
                resources={selected.kind === "twingate" ? resources : []}
                bound={boundVia(selected.kind)}
                onNavigate={onNavigate}
              />
            )
          )}
        </>
      )}

      <p className="text-body-secondary small mb-0">
        ParolaSSH observes VPN clients; it never starts, stops, or
        reconfigures them.
      </p>
    </div>
  );
}

/** Every client at a glance; each row opens that client's tab. */
function OverviewPane({
  installed,
  resources,
  bound,
  onSelect,
  onNavigate,
}: {
  installed: VpnStatus[];
  resources: VpnResource[];
  bound: BoundHost[];
  onSelect: (tab: VpnTab) => void;
  onNavigate: Navigate;
}) {
  const summaryFor = (status: VpnStatus) => {
    const parts = [status.detail];
    if (status.kind === "twingate" && resources.length > 0) {
      parts.push(`${resources.length} ${resources.length === 1 ? "resource" : "resources"}`);
    }
    const hostCount = bound.filter(
      ({ binding }) => binding.kind === status.kind || binding.kind === null,
    ).length;
    if (hostCount > 0) {
      parts.push(`${hostCount} ${hostCount === 1 ? "host" : "hosts"}`);
    }
    return parts.join(" · ");
  };

  return (
    <>
      <Card className="mb-4">
        <ListGroup variant="flush">
          {installed.map((status) => (
            <ListGroup.Item
              key={status.kind}
              action
              onClick={() => onSelect(status.kind)}
              className="d-flex align-items-center gap-2"
            >
              <StatusDot
                status={status.up ? "connected" : "offline"}
                title={`${VPN_LABELS[status.kind]} — ${status.detail}`}
              />
              <span className="fw-semibold">{VPN_LABELS[status.kind]}</span>
              <span className="text-body-secondary small me-auto">
                {summaryFor(status)}
              </span>
              <ChevronRight className="icon-sm text-body-secondary" aria-hidden="true" />
            </ListGroup.Item>
          ))}
        </ListGroup>
      </Card>

      <BoundHostsCard
        title="Hosts reached through a VPN"
        bound={bound}
        onNavigate={onNavigate}
      />
    </>
  );
}

/** One client's own pane: state line, its resources, its hosts. */
function ClientPane({
  status,
  resources,
  bound,
  onNavigate,
}: {
  status: VpnStatus;
  resources: VpnResource[];
  bound: BoundHost[];
  onNavigate: Navigate;
}) {
  const needingAuth = resources.filter((resource) => resource.needsAuth);

  return (
    <>
      <Card body className="mb-4">
        <div className="d-flex align-items-center gap-2">
          <span
            className={`status-dot status-dot--${status.up ? "connected" : "offline"}`}
            aria-hidden="true"
          />
          <span className="fw-semibold">{VPN_LABELS[status.kind]}</span>
          <span className="text-body-secondary small">{status.detail}</span>
        </div>
      </Card>

      {resources.length > 0 && (
        <Card className="mb-4">
          <Card.Header className="fw-semibold">Resources</Card.Header>
          <Table size="sm" responsive className="mb-0 align-middle">
            <thead>
              <tr>
                <th>Resource</th>
                <th>Address</th>
                <th>Alias</th>
                <th>Auth</th>
              </tr>
            </thead>
            <tbody>
              {resources.map((resource) => (
                <tr key={resource.name}>
                  <td>{resource.name}</td>
                  <td>
                    <code className="small">{resource.address}</code>
                  </td>
                  <td className="text-body-secondary">{resource.alias ?? "—"}</td>
                  <td>
                    <AuthPill resource={resource} />
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
          {needingAuth.length > 0 && (
            <Card.Footer className="small text-body-secondary">
              Re-authenticate with{" "}
              {needingAuth.map((resource, index) => (
                <span key={resource.name}>
                  {index > 0 && ", "}
                  <code>twingate auth "{resource.name}"</code>
                </span>
              ))}{" "}
              or from the Twingate app.
            </Card.Footer>
          )}
        </Card>
      )}

      <BoundHostsCard
        title={`Hosts through ${VPN_LABELS[status.kind]}`}
        bound={bound}
        onNavigate={onNavigate}
      />
    </>
  );
}

function BoundHostsCard({
  title,
  bound,
  onNavigate,
}: {
  title: string;
  bound: BoundHost[];
  onNavigate: Navigate;
}) {
  if (bound.length === 0) return null;

  return (
    <Card className="mb-4">
      <Card.Header className="fw-semibold">{title}</Card.Header>
      <Table size="sm" responsive className="mb-0 align-middle">
        <thead>
          <tr>
            <th>Host</th>
            <th>Address</th>
            <th>Via</th>
          </tr>
        </thead>
        <tbody>
          {bound.map(({ host, binding }) => (
            <tr key={host.id}>
              <td>
                <Button
                  variant="link"
                  size="sm"
                  className="p-0 text-decoration-none d-inline-flex align-items-center gap-2"
                  onClick={() => onNavigate({ kind: "host", hostId: host.id })}
                >
                  <StatusDot status={host.status} />
                  {host.label}
                </Button>
              </td>
              <td>
                <code className="small">{host.hostname}</code>
              </td>
              <td className="text-body-secondary">{binding.description}</td>
            </tr>
          ))}
        </tbody>
      </Table>
    </Card>
  );
}

/**
 * The auth column, compacted to one word so rows stay one line tall.
 *
 * "Auth expires in 4 days" becomes a green "4 days"; anything the backend
 * flagged as blocking becomes a red "expired"/"required". The client's full
 * wording survives in the tooltip.
 */
function AuthPill({ resource }: { resource: VpnResource }) {
  let variant = "";
  let text = resource.authStatus;

  if (resource.needsAuth) {
    variant = " status-badge--offline";
    text = resource.authStatus.toLowerCase().includes("expired")
      ? "expired"
      : "required";
  } else {
    const remaining = resource.authStatus.match(/expires in (.+)/i);
    if (remaining) {
      variant = " status-badge--connected";
      text = remaining[1];
    }
  }

  return (
    <span className={`status-badge${variant}`} title={resource.authStatus}>
      {text}
    </span>
  );
}
