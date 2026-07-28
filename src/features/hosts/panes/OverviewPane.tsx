import { Badge, Card } from "react-bootstrap";
import {
  Activity,
  Clock,
  Fingerprint,
  History,
  Network,
  ShieldAlert,
  ShieldCheck,
  Signal,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import { OsIcon } from "../OsIcon";
import { StatusBadge } from "../StatusIndicator";
import type { HostRow } from "../HostsProvider";
import {
  AUTH_METHOD_LABELS,
  ELEVATION_LABELS,
  OS_LABELS,
  type ConnectionInfo,
  type HostHealth,
} from "../types";
import { formatAbsolute, formatRelative } from "../../../lib/format";


export function OverviewPane({
  host,
  connection,
  health,
}: {
  host: HostRow;
  connection?: ConnectionInfo;
  health?: HostHealth;
}) {
  const blocked = connection?.elevation.kind === "unavailable";

  return (
    <div className="d-flex flex-column gap-3">
      {connection ? (
        <div className="stat-grid">
          <Stat
            label="Operating system"
            value={OS_LABELS[connection.os]}
            sub={connection.osDetail}
            glyph={<OsIcon os={connection.os} className="stat-tile__glyph" />}
          />
          <Stat
            label="Signed in as"
            value={connection.user || host.username}
            sub={AUTH_METHOD_LABELS[host.authMethod]}
            Icon={UserRound}
          />
          <Stat
            label="Connected"
            value={formatRelative(connection.connectedAt)}
            sub={formatAbsolute(connection.connectedAt)}
            Icon={Clock}
          />
          <Stat
            label="Heartbeat"
            value={health?.latencyMs != null ? `${health.latencyMs} ms` : "-"}
            sub={health ? "checked every 30s" : "not yet checked"}
            Icon={Activity}
          />
        </div>
      ) : (
        <div className="stat-grid">
          <Stat
            label="Status"
            value={<StatusBadge status={host.status} />}
            sub="Not connected"
            Icon={Signal}
          />
          <Stat
            label="Address"
            value={host.hostname}
            sub={`Port ${host.port}`}
            mono
            Icon={Network}
          />
          <Stat
            label="User"
            value={host.username}
            sub={AUTH_METHOD_LABELS[host.authMethod]}
            Icon={UserRound}
          />
          <Stat
            label="Last connected"
            value={formatRelative(host.lastConnected)}
            sub={formatAbsolute(host.lastConnected)}
            Icon={History}
          />
        </div>
      )}
      
      {connection && (
        <Card>
          <Card.Body className="d-flex gap-2">
            {blocked ? (
              <ShieldAlert className="icon-sm flex-shrink-0 mt-1 text-danger" aria-hidden="true" />
            ) : (
              <ShieldCheck className="icon-sm flex-shrink-0 mt-1 text-success" aria-hidden="true" />
            )}
            <div className="min-w-0">
              <div className="fw-semibold">
                {ELEVATION_LABELS[connection.elevation.kind]}
              </div>
              <div className="text-body-secondary small">
                {connection.elevationExplanation}
              </div>
              {connection.fingerprint && (
                <div className="mt-2">
                  <div className="detail-grid__label d-flex align-items-center gap-1">
                    <Fingerprint className="icon-sm" aria-hidden="true" />
                    Host key
                  </div>
                  <code className="small text-break user-select-auto">
                    {connection.fingerprint}
                  </code>
                </div>
              )}
            </div>
          </Card.Body>
        </Card>
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
    </div>
  );
}

function Stat({
  label,
  value,
  sub,
  mono,
  Icon,
  glyph,
}: {
  label: string;
  value: React.ReactNode;
  sub?: string | null;
  mono?: boolean;
  Icon?: LucideIcon;
  glyph?: React.ReactNode;
}) {
  return (
    <div className="stat-tile">
      <div className="stat-tile__label">
        {glyph ?? (Icon && <Icon className="stat-tile__glyph" aria-hidden="true" />)}
        {label}
      </div>
      <div className={`stat-tile__value${mono ? " font-monospace" : ""}`}>{value}</div>
      {sub && <div className="stat-tile__sub">{sub}</div>}
    </div>
  );
}

export function OsBadge({ connection }: { connection?: ConnectionInfo }) {
  if (!connection) return null;
  return (
    <Badge bg="secondary" className="d-inline-flex align-items-center gap-1">
      <OsIcon os={connection.os} />
      {OS_LABELS[connection.os]}
    </Badge>
  );
}
