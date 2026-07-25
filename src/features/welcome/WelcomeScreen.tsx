import { Button, Col, Row } from "react-bootstrap";
import { ChevronRight, Download, History, Plus, Server } from "lucide-react";
import { useHosts } from "../hosts/HostsProvider";
import { StatusDot } from "../hosts/StatusIndicator";
import type { SshHost } from "../hosts/types";
import { formatRelative } from "../../lib/format";
import type { Navigate } from "../../navigation";

const RECENT_LIMIT = 6;

export function WelcomeScreen({ onNavigate }: { onNavigate: Navigate }) {
  const { hosts, groups, recent, onlineCount, connect } = useHosts();
  const recentHosts = recent.slice(0, RECENT_LIMIT);

  return (
    <div className="page">
      <header className="welcome-hero mb-4">
        <h1 className="welcome-hero__title">{greeting()}</h1>
        <p className="text-body-secondary mb-0">
          {hosts.length} saved {hosts.length === 1 ? "host" : "hosts"} ·{" "}
          {onlineCount} online · {groups.length}{" "}
          {groups.length === 1 ? "group" : "groups"}
        </p>
      </header>

      <div className="d-flex flex-wrap gap-2 mb-5">
        <Button variant="primary">
          <Plus aria-hidden="true" />
          New connection
        </Button>
        <Button variant="outline-secondary" disabled>
          <Download aria-hidden="true" />
          Import ~/.ssh/config
        </Button>
        <Button
          variant="outline-secondary"
          onClick={() => onNavigate({ kind: "hosts" })}
        >
          <Server aria-hidden="true" />
          Browse all hosts
        </Button>
      </div>

      <div className="d-flex align-items-center gap-2 mb-3">
        <h2 className="section-title">Recent</h2>
        <Button
          variant="link"
          size="sm"
          className="ms-auto p-0 text-decoration-none"
          onClick={() => onNavigate({ kind: "hosts" })}
        >
          View all
          <ChevronRight className="icon-sm" aria-hidden="true" />
        </Button>
      </div>

      {recentHosts.length === 0 ? (
        <EmptyRecent />
      ) : (
        <Row xs={1} md={2} xl={3} className="g-3">
          {recentHosts.map((host) => (
            <Col key={host.id}>
              <RecentHostCard
                host={host}
                onOpen={() => onNavigate({ kind: "host", hostId: host.id })}
                onConnect={() => connect(host)}
              />
            </Col>
          ))}
        </Row>
      )}
    </div>
  );
}

function RecentHostCard({
  host,
  onOpen,
  onConnect,
}: {
  host: SshHost;
  onOpen: () => void;
  onConnect: () => void;
}) {
  return (
    <div
      className="host-card"
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpen();
        }
      }}
    >
      <div className="d-flex align-items-center gap-2">
        <StatusDot status={host.status} />
        <span className="host-card__name flex-grow-1">{host.label}</span>
        <span className="tag-chip">{host.group}</span>
      </div>

      <div className="host-card__target">
        {host.username}@{host.hostname}
        {host.port !== 22 && `:${host.port}`}
      </div>

      <div className="d-flex align-items-center gap-2 mt-auto">
        <small className="text-body-secondary flex-grow-1 d-inline-flex align-items-center gap-1">
          <History className="icon-sm" aria-hidden="true" />
          {formatRelative(host.lastConnected)}
        </small>
        <Button
          size="sm"
          variant="primary"
          onClick={(event) => {
            event.stopPropagation();
            onConnect();
          }}
        >
          Connect
        </Button>
      </div>
    </div>
  );
}

function EmptyRecent() {
  return (
    <div className="border rounded text-center text-body-secondary py-5">
      <History className="icon-xl mb-2" aria-hidden="true" />
      <div>
        No recent connections yet. Connect to a host and it will show up here.
      </div>
    </div>
  );
}

function greeting() {
  const hour = new Date().getHours();
  if (hour < 5) return "Still up?";
  if (hour < 12) return "Good morning";
  if (hour < 18) return "Good afternoon";
  return "Good evening";
}
