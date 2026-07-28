import { useMemo, useState, useSyncExternalStore } from "react";
import { Collapse, Form, InputGroup } from "react-bootstrap";
import {
  AppWindow,
  ArrowUpDown,
  ChevronDown,
  House,
  Info,
  KeyRound,
  Search,
  Server,
  Settings,
  Shield,
} from "lucide-react";
import { useHosts } from "../features/hosts/HostsProvider";
import { StatusDot } from "../features/hosts/StatusIndicator";
import { useHostActions } from "../features/hosts/useHostActions";
import { useHostContextMenu } from "../features/hosts/useHostContextMenu";
import * as terminals from "../features/hosts/terminalStore";
import * as transfers from "../features/transfers/transferStore";
import { AnimatedValue } from "./AnimatedValue";
import { useKeys } from "../features/keys/KeysProvider";
import type { SshHost } from "../features/hosts/types";
import type { Navigate, View } from "../navigation";

type HostSidebarProps = {
  view: View;
  onNavigate: Navigate;
  hidden: boolean;
};

export function HostSidebar({ view, onNavigate, hidden }: HostSidebarProps) {
  const { hosts, groups, connectedCount } = useHosts();
  const { report } = useKeys();

  // Shells open and close from the terminal pane, which lives elsewhere in
  // the tree - subscribe to the store rather than lifting its state.
  useSyncExternalStore(terminals.subscribe, terminals.getVersion);
  const sessionCount = terminals.liveCount();

  // Same reason, one store over: a transfer queued from a host pane must show
  // up here even though nothing in this tree started it.
  useSyncExternalStore(transfers.subscribe, transfers.getVersion);
  const transferCount = transfers.pendingCount();
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  // The sidebar is the fastest route to a host, so connect/power/edit are
  // reachable from it by right-click rather than only from the hosts table.
  // It owns its own dialog set - the table's live on a page that may not be
  // mounted when the menu is used.
  const { actions, dialogs } = useHostActions({
    onOpenTerminal: (host) => onNavigate({ kind: "host", hostId: host.id }),
  });
  const { openContextMenu, contextMenu } = useHostContextMenu({
    actions,
    onOpen: (host) => onNavigate({ kind: "host", hostId: host.id }),
  });

  // Only critical and high findings earn a badge - anything more and the
  // count stops meaning "look at this now".
  const keyAlerts = report ? report.counts.critical + report.counts.high : 0;

  const visibleGroups = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return groups;

    return groups
      .map((group) => ({
        ...group,
        hosts: group.hosts.filter((host) => matchesHost(host, needle)),
      }))
      .filter((group) => group.hosts.length > 0);
  }, [groups, query]);

  // While searching, show every match rather than honouring collapsed state.
  const searching = query.trim().length > 0;

  const toggleGroup = (name: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  return (
    <aside
      className={`app-sidebar${hidden ? " is-hidden" : ""}`}
      aria-label="Hosts"
      aria-hidden={hidden}
    >
      <div className="app-sidebar__header">
        <InputGroup size="sm">
          <InputGroup.Text>
            <Search className="icon-sm" aria-hidden="true" />
          </InputGroup.Text>
          <Form.Control
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter hosts"
            aria-label="Filter hosts"
          />
        </InputGroup>
      </div>

      <div className="app-sidebar__scroll">
        <button
          type="button"
          className={`sidebar-item${view.kind === "welcome" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "welcome" })}
        >
          <House aria-hidden="true" />
          <span className="sidebar-item__label">Home</span>
        </button>

        <button
          type="button"
          className={`sidebar-item${view.kind === "hosts" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "hosts" })}
        >
          <Server aria-hidden="true" />
          <span className="sidebar-item__label">All hosts</span>
          <span className="sidebar-item__meta">{hosts.length}</span>
        </button>

        <div className="sidebar-label">
          <span className="flex-grow-1">Hosts</span>
          <span className="fw-normal text-body-secondary">
            <AnimatedValue value={connectedCount} />/{hosts.length} connected
          </span>
        </div>

        {visibleGroups.length === 0 && (
          <p className="px-2 py-3 mb-0 small text-body-secondary">
            No hosts match “{query}”.
          </p>
        )}

        {visibleGroups.map((group) => {
          const isCollapsed = !searching && collapsed.has(group.name);

          return (
            <div key={group.name}>
              <button
                type="button"
                className="sidebar-item"
                onClick={() => toggleGroup(group.name)}
                aria-expanded={!isCollapsed}
              >
                <ChevronDown
                  className={`icon-sm sidebar-caret${
                    isCollapsed ? " is-collapsed" : ""
                  }`}
                  aria-hidden="true"
                />
                <span className="sidebar-item__label fw-semibold">
                  {group.name}
                </span>
                <span className="sidebar-item__meta">
                  <AnimatedValue value={group.connectedCount} />/{group.hosts.length}
                </span>
              </button>

              <Collapse in={!isCollapsed}>
                <div className="sidebar-group__items">
                  {group.hosts.map((host) => (
                    <button
                      key={host.id}
                      type="button"
                      className={`sidebar-item${
                        view.kind === "host" && view.hostId === host.id
                          ? " is-active"
                          : ""
                      }`}
                      onClick={() => onNavigate({ kind: "host", hostId: host.id })}
                      onContextMenu={(event) => openContextMenu(host, event)}
                      title={`${host.username}@${host.hostname}:${host.port}`}
                    >
                      <StatusDot status={host.status} />
                      <span className="sidebar-item__label">
                        {host.label}
                        <span className="sidebar-host__sub">{host.hostname}</span>
                      </span>
                    </button>
                  ))}
                </div>
              </Collapse>
            </div>
          );
        })}
      </div>

      <div className="app-sidebar__footer">
        <button
          type="button"
          className={`sidebar-item${
            view.kind === "keys" || view.kind === "key" || view.kind === "audit"
              ? " is-active"
              : ""
          }`}
          onClick={() => onNavigate({ kind: "keys" })}
        >
          <KeyRound aria-hidden="true" />
          <span className="sidebar-item__label">Keys</span>
          {keyAlerts > 0 && (
            <span className="sidebar-item__meta sidebar-item__meta--danger">
              {keyAlerts}
            </span>
          )}
        </button>

        <button
          type="button"
          className={`sidebar-item${view.kind === "vpn" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "vpn" })}
        >
          <Shield aria-hidden="true" />
          <span className="sidebar-item__label">VPN</span>
        </button>

        <button
          type="button"
          className={`sidebar-item${view.kind === "sessions" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "sessions" })}
        >
          <AppWindow aria-hidden="true" />
          <span className="sidebar-item__label">Sessions</span>
          {sessionCount > 0 && (
            <span className="sidebar-item__meta">
              <AnimatedValue value={sessionCount} />
            </span>
          )}
        </button>

        <button
          type="button"
          className={`sidebar-item${view.kind === "transfers" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "transfers" })}
        >
          <ArrowUpDown aria-hidden="true" />
          <span className="sidebar-item__label">Transfers</span>
          {transferCount > 0 && (
            <span className="sidebar-item__meta">
              <AnimatedValue value={transferCount} />
            </span>
          )}
        </button>

        <button
          type="button"
          className={`sidebar-item${view.kind === "settings" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "settings" })}
        >
          <Settings aria-hidden="true" />
          <span className="sidebar-item__label">Settings</span>
        </button>

        <button
          type="button"
          className={`sidebar-item${view.kind === "about" ? " is-active" : ""}`}
          onClick={() => onNavigate({ kind: "about" })}
        >
          <Info aria-hidden="true" />
          <span className="sidebar-item__label">About</span>
        </button>
      </div>

      {contextMenu}
      {dialogs}
    </aside>
  );
}

function matchesHost(host: SshHost, needle: string) {
  return (
    host.label.toLowerCase().includes(needle) ||
    host.hostname.toLowerCase().includes(needle) ||
    host.username.toLowerCase().includes(needle) ||
    host.tags.some((tag) => tag.toLowerCase().includes(needle))
  );
}
