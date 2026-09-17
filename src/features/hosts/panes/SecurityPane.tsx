import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Card, Form, Spinner, Table } from "react-bootstrap";
import {
  BrickWall,
  Info,
  Maximize2,
  Minimize2,
  Network,
  RefreshCw,
  ShieldCheck,
  ShieldEllipsis,
  ShieldOff,
  Users,
} from "lucide-react";
import { Segmented } from "../../../components/Segmented";
import * as api from "../api";
import { errorMessage } from "../api";
import { useElevation } from "../ElevationProvider";
import { useHosts } from "../HostsProvider";
import { AuditPane } from "./AuditPane";
import type {
  FirewallBackend,
  FirewallReport,
  FirewallState,
  PortsReport,
  SecurityView,
  UsersReport,
} from "../types";

type Section = "audit" | "ports" | "firewall" | "users";

const SECTIONS = [
  { value: "audit" as const, label: "Audit", Icon: ShieldCheck },
  { value: "ports" as const, label: "Ports", Icon: Network },
  { value: "firewall" as const, label: "Firewall", Icon: BrickWall },
  { value: "users" as const, label: "Users", Icon: Users },
];

export function SecurityPane({ hostId }: { hostId: string }) {
  const [section, setSection] = useState<Section>("audit");

  useEffect(() => setSection("audit"), [hostId]);

  return (
    <div className="d-flex flex-column gap-3">
      <div>
        <Segmented
          value={section}
          options={SECTIONS}
          onChange={setSection}
          label="Security section"
        />
      </div>

      {section === "audit" ? (
        <AuditPane hostId={hostId} />
      ) : section === "ports" ? (
        <PortsSection hostId={hostId} />
      ) : section === "firewall" ? (
        <FirewallSection hostId={hostId} />
      ) : (
        <UsersSection hostId={hostId} />
      )}
    </div>
  );
}

/** Loads a view unprivileged on open, with an opt-in sudo re-read. Once sudo
 *  is granted, Refresh stays elevated until dropped or the section closes.
 *  A typed password is never held here: Rust keeps it on the session. */
function useSecurityRead<T>(
  hostId: string,
  view: SecurityView,
  summary: string,
  load: (elevate: boolean, password: string | null) => Promise<T>,
) {
  const { getConnection } = useHosts();
  const requestElevation = useElevation();
  const connection = getConnection(hostId);

  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Root needs no sudo and Windows has none; only a sudo route earns the button.
  const canSudo =
    connection !== undefined &&
    connection.os !== "windows" &&
    (connection.elevation.kind === "sudoPassword" ||
      connection.elevation.kind === "sudoNoPassword");

  // Only whether the last grant succeeded; the password itself stays in Rust.
  const elevatedRef = useRef(false);

  const read = useCallback(
    async (elevate: boolean, password: string | null) => {
      setLoading(true);
      setError(null);
      try {
        setData(await load(elevate, password));
        return true;
      } catch (caught) {
        setError(errorMessage(caught));
        return false;
      } finally {
        setLoading(false);
      }
    },
    [load],
  );

  useEffect(() => {
    elevatedRef.current = false;
    setData(null);
    void read(false, null);
  }, [read]);

  const refresh = () => void read(elevatedRef.current, null);

  const dropSudo = () => {
    elevatedRef.current = false;
    void read(false, null);
  };

  const readWithSudo = async () => {
    let command: string | null = null;
    try {
      command = await api.previewSecurityCommand(hostId, view);
    } catch (caught) {
      setError(errorMessage(caught));
      return;
    }
    const grant = await requestElevation({ hostId, summary, command });
    if (grant.outcome !== "granted") return;
    elevatedRef.current = await read(true, grant.password);
  };

  return { data, loading, error, canSudo, refresh, readWithSudo, dropSudo };
}

function Toolbar({
  loading,
  canSudo,
  elevated,
  onRefresh,
  onSudo,
  onDropSudo,
  children,
}: {
  loading: boolean;
  canSudo: boolean;
  elevated: boolean;
  onRefresh: () => void;
  onSudo: () => void;
  onDropSudo: () => void;
  children?: React.ReactNode;
}) {
  return (
    <div className="d-flex flex-wrap align-items-center gap-2">
      {children}
      <span className="me-auto" />
      {elevated && <span className="status-badge status-badge--warning">read as root</span>}
      {canSudo &&
        (elevated ? (
          <Button size="sm" variant="outline-secondary" disabled={loading} onClick={onDropSudo}>
            <ShieldOff className="icon-sm" aria-hidden="true" />
            Read without sudo
          </Button>
        ) : (
          <Button size="sm" variant="outline-secondary" disabled={loading} onClick={onSudo}>
            <ShieldEllipsis className="icon-sm" aria-hidden="true" />
            Read with sudo
          </Button>
        ))}
      <Button size="sm" variant="outline-secondary" disabled={loading} onClick={onRefresh}>
        <RefreshCw className="icon-sm" aria-hidden="true" />
        Refresh
      </Button>
    </div>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="d-flex align-items-center gap-2 text-body-secondary py-4">
      <Spinner animation="border" size="sm" aria-hidden="true" />
      {label}
    </div>
  );
}

function Note({ text }: { text: string | null }) {
  if (!text) return null;
  return (
    <Alert variant="secondary" className="d-flex gap-2 small py-2 mb-0">
      <Info className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
      <div>{text}</div>
    </Alert>
  );
}

const WILDCARD_ADDRESSES = new Set(["*", "0.0.0.0", "::"]);

function PortsSection({ hostId }: { hostId: string }) {
  const load = useCallback(
    (elevate: boolean, password: string | null) =>
      api.listListeningPorts(hostId, elevate, password),
    [hostId],
  );
  const { data, loading, error, canSudo, refresh, readWithSudo, dropSudo } =
    useSecurityRead<PortsReport>(hostId, "ports", "List listening ports as root", load);
  const [filter, setFilter] = useState("");

  const needle = filter.trim().toLowerCase();
  const visible = (data?.ports ?? []).filter(
    (port) =>
      !needle ||
      String(port.port).includes(needle) ||
      port.address.toLowerCase().includes(needle) ||
      (port.process ?? "").toLowerCase().includes(needle),
  );

  return (
    <div className="d-flex flex-column gap-3">
      <Toolbar
        loading={loading}
        canSudo={canSudo}
        elevated={Boolean(data?.elevated)}
        onRefresh={refresh}
        onSudo={() => void readWithSudo()}
        onDropSudo={dropSudo}
      >
        <Form.Control
          type="search"
          className="w-auto flex-grow-1"
          style={{ maxWidth: "24rem" }}
          placeholder="Filter by port, address, or process…"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        {data && (
          <span className="text-body-secondary small">
            {visible.length} of {data.ports.length} via <code>{data.tool}</code>
          </span>
        )}
      </Toolbar>

      {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

      {loading && !data ? (
        <Loading label="Reading listening sockets…" />
      ) : (
        data && (
          <>
            <Note text={data.note} />
            <div className="services-table">
              <Table hover size="sm" className="align-middle mb-0">
                <thead>
                  <tr>
                    <th style={{ width: "5rem" }}>Proto</th>
                    <th style={{ width: "6rem" }}>Port</th>
                    <th>Address</th>
                    <th>Process</th>
                    <th style={{ width: "6rem" }}>PID</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map((port) => (
                    <tr key={`${port.proto}|${port.address}|${port.port}|${port.pid ?? ""}`}>
                      <td className="text-uppercase small">{port.proto}</td>
                      <td className="font-monospace small">{port.port}</td>
                      <td className="font-monospace small">
                        {port.address}
                        {WILDCARD_ADDRESSES.has(port.address) && (
                          <span className="text-body-secondary ms-2">all interfaces</span>
                        )}
                      </td>
                      <td className="small">
                        {port.process ?? <span className="text-body-secondary">-</span>}
                      </td>
                      <td className="font-monospace small text-body-secondary">
                        {port.pid ?? "-"}
                      </td>
                    </tr>
                  ))}
                  {visible.length === 0 && (
                    <tr>
                      <td colSpan={5} className="text-body-secondary py-3">
                        {data.ports.length === 0
                          ? "No listening sockets were reported."
                          : `No port matches “${filter}”.`}
                      </td>
                    </tr>
                  )}
                </tbody>
              </Table>
            </div>
          </>
        )
      )}
    </div>
  );
}

const FIREWALL_BADGE: Record<FirewallState, { className: string; label: string }> = {
  active: { className: "status-badge status-badge--connected", label: "Active" },
  inactive: { className: "status-badge status-badge--warning", label: "Inactive" },
  unknown: { className: "status-badge", label: "Unknown" },
};

function FirewallSection({ hostId }: { hostId: string }) {
  const load = useCallback(
    (elevate: boolean, password: string | null) => api.readFirewall(hostId, elevate, password),
    [hostId],
  );
  const { data, loading, error, canSudo, refresh, readWithSudo, dropSudo } =
    useSecurityRead<FirewallReport>(hostId, "firewall", "Read firewall rules as root", load);

  return (
    <div className="d-flex flex-column gap-3">
      <Toolbar
        loading={loading}
        canSudo={canSudo}
        elevated={Boolean(data?.elevated)}
        onRefresh={refresh}
        onSudo={() => void readWithSudo()}
        onDropSudo={dropSudo}
      />

      {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

      {loading && !data ? (
        <Loading label="Reading firewall state…" />
      ) : (
        data && (
          <>
            <Note text={data.note} />
            {data.backends.map((backend) => (
              <FirewallCard key={backend.name} backend={backend} />
            ))}
          </>
        )
      )}
    </div>
  );
}

function FirewallCard({ backend }: { backend: FirewallBackend }) {
  const [expanded, setExpanded] = useState(false);
  const badge = FIREWALL_BADGE[backend.state];

  return (
    <Card>
      <Card.Body>
        <div className="d-flex flex-wrap align-items-center gap-2 mb-2">
          <h2 className="h6 mb-0">{backend.name}</h2>
          <span className={badge.className}>{badge.label}</span>
          {backend.summary && (
            <span className="text-body-secondary small">{backend.summary}</span>
          )}
          {backend.output && (
            <Button
              size="sm"
              variant="link"
              className="ms-auto p-0 text-decoration-none text-body-secondary"
              onClick={() => setExpanded((value) => !value)}
              aria-expanded={expanded}
            >
              {expanded ? (
                <Minimize2 className="icon-sm" aria-hidden="true" />
              ) : (
                <Maximize2 className="icon-sm" aria-hidden="true" />
              )}
              {expanded ? "Collapse" : "Expand"}
            </Button>
          )}
        </div>
        <pre
          className={`command-output user-select-auto mb-0${
            expanded ? " command-output--expanded" : ""
          }`}
        >
          {backend.output || "No rules to show."}
        </pre>
      </Card.Body>
    </Card>
  );
}

function UsersSection({ hostId }: { hostId: string }) {
  const [data, setData] = useState<UsersReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await api.listLoggedInUsers(hostId));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    setData(null);
    void refresh();
  }, [refresh]);

  const showState = (data?.users ?? []).some((user) => user.state !== null);

  return (
    <div className="d-flex flex-column gap-3">
      <Toolbar
        loading={loading}
        canSudo={false}
        elevated={false}
        onRefresh={() => void refresh()}
        onSudo={() => undefined}
        onDropSudo={() => undefined}
      >
        {data && (
          <span className="text-body-secondary small">
            {data.users.length} {data.users.length === 1 ? "session" : "sessions"}
          </span>
        )}
      </Toolbar>

      {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

      {loading && !data ? (
        <Loading label="Reading logged-in users…" />
      ) : (
        data && (
          <>
            <Note text={data.note} />
            <div className="services-table">
              <Table hover size="sm" className="align-middle mb-0">
                <thead>
                  <tr>
                    <th>User</th>
                    <th>Terminal</th>
                    <th>From</th>
                    <th>Login time</th>
                    {showState && <th>State</th>}
                    {showState && <th>Idle</th>}
                  </tr>
                </thead>
                <tbody>
                  {data.users.map((user, index) => (
                    <tr key={`${user.user}|${user.terminal ?? ""}|${index}`}>
                      <td className="small fw-medium">{user.user}</td>
                      <td className="font-monospace small">{user.terminal ?? "-"}</td>
                      <td className="font-monospace small">{user.from ?? "-"}</td>
                      <td className="small text-body-secondary">{user.loginTime ?? "-"}</td>
                      {showState && <td className="small">{user.state ?? "-"}</td>}
                      {showState && <td className="small">{user.idle ?? "-"}</td>}
                    </tr>
                  ))}
                  {data.users.length === 0 && (
                    <tr>
                      <td colSpan={showState ? 6 : 4} className="text-body-secondary py-3">
                        Nobody is logged in.
                      </td>
                    </tr>
                  )}
                </tbody>
              </Table>
            </div>
          </>
        )
      )}
    </div>
  );
}
