import { useSyncExternalStore } from "react";
import { Button, Card, ListGroup } from "react-bootstrap";
import { ChevronRight, Plus, SquareTerminal, Unplug, X } from "lucide-react";
import { useHosts, type HostRow } from "../hosts/HostsProvider";
import { StatusDot } from "../hosts/StatusIndicator";
import * as terminals from "../hosts/terminalStore";
import { useHostActions } from "../hosts/useHostActions";
import type { Navigate } from "../../navigation";

type ShellRow = { shellId: number; title: string; exited: boolean; exitCode: number | null };
type HostSessions = { host: HostRow; shells: ShellRow[] };

/**
 * Every open shell in one place. Terminal tabs live inside a host's own pane,
 * so without this the only way to find a shell you left running is to
 * remember which host it was on.
 */
export function SessionsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { hosts } = useHosts();
  const { actions, dialogs } = useHostActions();

  useSyncExternalStore(terminals.subscribe, terminals.getVersion);
  const open = terminals.all();

  // Connected hosts are listed even with no shell - that is where you go to
  // start one. Hosts that dropped keep their tabs until those are closed.
  const rows: HostSessions[] = hosts
    .map((host) => ({
      host,
      shells: open
        .filter((entry) => entry.hostId === host.id)
        .map(({ shellId, title, exited, exitCode }) => ({
          shellId,
          title,
          exited,
          exitCode,
        })),
    }))
    .filter(({ host, shells }) => host.status === "connected" || shells.length > 0);

  const live = open.filter((entry) => !entry.exited).length;
  const hostCount = rows.filter(({ shells }) => shells.length > 0).length;

  return (
    <div className="page">
      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">Sessions</h1>
          <p className="text-body-secondary mb-0">
            {live === 0
              ? "No shells open"
              : `${live} ${live === 1 ? "shell" : "shells"} on ${hostCount} ${
                  hostCount === 1 ? "host" : "hosts"
                }`}
          </p>
        </div>
      </header>

      {rows.length === 0 ? (
        <Card>
          <Card.Body className="text-center py-5">
            <p className="text-body-secondary mb-3">
              Nothing is running. Connect to a host and open a terminal - every
              shell you start shows up here.
            </p>
            <Button variant="primary" onClick={() => onNavigate({ kind: "hosts" })}>
              All hosts
            </Button>
          </Card.Body>
        </Card>
      ) : (
        <div className="d-flex flex-column gap-3">
          {rows.map(({ host, shells }) => (
            <Card key={host.id}>
              <Card.Body className="pb-2">
                <div className="d-flex flex-wrap align-items-center gap-2 mb-2">
                  <StatusDot status={host.status} />
                  <button
                    type="button"
                    className="session-host__name"
                    onClick={() => onNavigate({ kind: "host", hostId: host.id })}
                  >
                    {host.label}
                    <ChevronRight className="icon-sm" aria-hidden="true" />
                  </button>
                  <span className="text-body-secondary font-monospace small me-auto">
                    {host.username}@{host.hostname}:{host.port}
                  </span>

                  <Button
                    size="sm"
                    variant="outline-secondary"
                    onClick={() =>
                      onNavigate({ kind: "host", hostId: host.id, feature: "terminal" })
                    }
                  >
                    <Plus aria-hidden="true" />
                    New terminal
                  </Button>
                  {host.status === "connected" && (
                    <Button
                      size="sm"
                      variant="outline-secondary"
                      onClick={() => void actions.onDisconnect(host)}
                      title="Close the SSH session and every shell on it"
                    >
                      <Unplug aria-hidden="true" />
                      Disconnect
                    </Button>
                  )}
                </div>

                {shells.length === 0 ? (
                  <p className="text-body-secondary small mb-2">
                    Connected, no shell open.
                  </p>
                ) : (
                  <ListGroup variant="flush">
                    {shells.map((shell) => (
                      <ListGroup.Item
                        key={shell.shellId}
                        className="d-flex align-items-center gap-2 px-0"
                      >
                        <SquareTerminal
                          className="icon-sm text-body-secondary"
                          aria-hidden="true"
                        />
                        <span className="me-auto">
                          {shell.title}
                          {shell.exited && (
                            <span className="session-shell__exit">
                              {shell.exitCode === null
                                ? "ended"
                                : `exit ${shell.exitCode}`}
                            </span>
                          )}
                        </span>

                        <Button
                          size="sm"
                          variant="link"
                          className="p-0 text-decoration-none"
                          onClick={() =>
                            onNavigate({
                              kind: "host",
                              hostId: host.id,
                              feature: "terminal",
                              shellId: shell.shellId,
                            })
                          }
                        >
                          Open
                        </Button>
                        <Button
                          size="sm"
                          variant="outline-secondary"
                          onClick={() => void terminals.close(shell.shellId)}
                          aria-label={`Close ${shell.title} on ${host.label}`}
                        >
                          <X aria-hidden="true" />
                        </Button>
                      </ListGroup.Item>
                    ))}
                  </ListGroup>
                )}
              </Card.Body>
            </Card>
          ))}
        </div>
      )}

      {dialogs}
    </div>
  );
}
