import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import * as api from "./api";
import { errorMessage } from "./api";
import type {
  ConnectionInfo,
  HostDraft,
  HostHealth,
  HostStatus,
  PowerOutcome,
  PowerRequest,
  ProbeResult,
  SshHost,
} from "./types";

/** How often every saved host is checked for liveness. */
const HEARTBEAT_INTERVAL_MS = 30_000;

/** A saved host plus whatever we currently know about reaching it. */
export type HostRow = SshHost & { status: HostStatus };

export type HostGroup = {
  name: string;
  hosts: HostRow[];
  onlineCount: number;
};

type ConnectOptions = {
  password?: string | null;
  remember?: boolean;
  trustUnknown?: boolean;
};

type HostsContextValue = {
  hosts: HostRow[];
  groups: HostGroup[];
  /** Most recently connected first; never-connected hosts excluded. */
  recent: HostRow[];
  onlineCount: number;
  loading: boolean;
  error: string | null;

  getHost: (id: string) => HostRow | undefined;
  /** Live session details, or undefined when not connected. */
  getConnection: (id: string) => ConnectionInfo | undefined;
  /** Last heartbeat result, or undefined before the first one. */
  getHealth: (id: string) => HostHealth | undefined;

  refresh: () => Promise<void>;
  save: (draft: HostDraft) => Promise<SshHost>;
  remove: (id: string) => Promise<void>;

  probe: (hostname: string, port: number) => Promise<ProbeResult>;
  connect: (id: string, options?: ConnectOptions) => Promise<ConnectionInfo>;
  disconnect: (id: string) => Promise<void>;
  power: (
    id: string,
    request: PowerRequest,
    password?: string | null,
  ) => Promise<PowerOutcome>;
};

const HostsContext = createContext<HostsContextValue | null>(null);

export function HostsProvider({ children }: { children: ReactNode }) {
  const [hosts, setHosts] = useState<SshHost[]>([]);
  const [connections, setConnections] = useState<Record<string, ConnectionInfo>>({});
  /** Last heartbeat result per host. Absent means we have not looked yet. */
  const [health, setHealth] = useState<Record<string, HostHealth>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Guards against a slow load overwriting a newer one.
  const requestId = useRef(0);

  const refresh = useCallback(async () => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);

    try {
      const [nextHosts, connectedIds] = await Promise.all([
        api.listHosts(),
        api.connectedHosts(),
      ]);

      if (id !== requestId.current) return;
      setHosts(nextHosts);

      // A session the Rust side has dropped (an immediate reboot, say) must
      // not linger in the map as a connected host.
      setConnections((previous) => {
        const next: Record<string, ConnectionInfo> = {};
        for (const hostId of connectedIds) {
          if (previous[hostId]) next[hostId] = previous[hostId];
        }
        return next;
      });
    } catch (caught) {
      if (id !== requestId.current) return;
      setError(errorMessage(caught));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = useCallback(
    async (draft: HostDraft) => {
      const saved = await api.saveHost(draft);
      await refresh();
      return saved;
    },
    [refresh],
  );

  // `remove` needs the current connections without re-creating itself every
  // time one changes, which would remount every row's handlers.
  const connectionsRef = useRef(connections);
  useEffect(() => {
    connectionsRef.current = connections;
  }, [connections]);

  const remove = useCallback(
    async (id: string) => {
      // Drop the session first: a deleted host with a live connection would
      // be unreachable from the UI but still holding a socket open.
      if (connectionsRef.current[id]) {
        await api.disconnectHost(id).catch(() => undefined);
      }
      await api.deleteHost(id);
      setConnections(({ [id]: _removed, ...rest }) => rest);
      await refresh();
    },
    [refresh],
  );

  const probe = useCallback(async (hostname: string, port: number) => {
    return api.probeHost(hostname, port);
  }, []);

  /**
   * Poll every host on a timer so the list reflects reality without anyone
   * clicking anything.
   *
   * The Rust side drops sessions that fail their liveness check, so a host
   * that rebooted out from under us stops claiming to be connected here too.
   * The timer pauses while the window is hidden: a backgrounded app has no
   * reason to keep opening sockets, and resuming runs one immediately so the
   * list is never stale by more than a moment after you come back.
   */
  useEffect(() => {
    if (hosts.length === 0) return;

    let cancelled = false;
    let timer: number | undefined;

    const beat = async () => {
      if (document.hidden) return;
      try {
        const results = await api.heartbeat();
        if (cancelled) return;

        setHealth(Object.fromEntries(results.map((entry) => [entry.hostId, entry])));

        // Reap sessions the Rust side just dropped.
        const stillConnected = new Set(
          results.filter((entry) => entry.connected).map((entry) => entry.hostId),
        );
        setConnections((previous) => {
          const next: Record<string, ConnectionInfo> = {};
          for (const [hostId, info] of Object.entries(previous)) {
            if (stillConnected.has(hostId)) next[hostId] = info;
          }
          return Object.keys(next).length === Object.keys(previous).length
            ? previous
            : next;
        });
      } catch {
        // A failed heartbeat is not worth an error banner — the next one is
        // thirty seconds away, and the statuses simply go stale until then.
      }
    };

    void beat();
    timer = window.setInterval(beat, HEARTBEAT_INTERVAL_MS);

    const onVisibility = () => {
      if (!document.hidden) void beat();
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [hosts.length]);

  const connect = useCallback(async (id: string, options: ConnectOptions = {}) => {
    try {
      const info = await api.connectHost(id, options);
      setConnections((previous) => ({ ...previous, [id]: info }));
      return info;
    } catch (caught) {
      // A failed connection is evidence about the host, so record it rather
      // than leaving the row sitting at "unknown".
      setHealth((previous) => ({
        ...previous,
        [id]: { hostId: id, connected: false, reachable: false, latencyMs: null },
      }));
      throw caught;
    }
  }, []);

  const disconnect = useCallback(async (id: string) => {
    await api.disconnectHost(id);
    setConnections(({ [id]: _removed, ...rest }) => rest);
  }, []);

  const power = useCallback(
    async (id: string, request: PowerRequest, password?: string | null) => {
      const outcome = await api.powerHost(id, request, password);

      // An immediate shutdown or reboot takes the session with it, so the
      // Rust side has already dropped it — mirror that here.
      const terminal =
        outcome.succeeded && request.action !== "cancel" && request.delayMinutes === 0;
      if (terminal) {
        setConnections(({ [id]: _removed, ...rest }) => rest);
        setHealth((previous) => ({
          ...previous,
          [id]: { hostId: id, connected: false, reachable: false, latencyMs: null },
        }));
      }

      return outcome;
    },
    [],
  );

  const value = useMemo<HostsContextValue>(() => {
    const rows: HostRow[] = hosts.map((host) => ({
      ...host,
      status: statusFor(host.id, connections, health),
    }));

    const byGroup = new Map<string, HostRow[]>();
    for (const host of rows) {
      const bucket = byGroup.get(host.group);
      if (bucket) bucket.push(host);
      else byGroup.set(host.group, [host]);
    }

    const groups: HostGroup[] = [...byGroup.entries()]
      .map(([name, groupHosts]) => ({
        name,
        hosts: [...groupHosts].sort((a, b) => a.label.localeCompare(b.label)),
        onlineCount: groupHosts.filter((host) => host.status === "online").length,
      }))
      .sort((a, b) => a.name.localeCompare(b.name));

    const recent = rows
      .filter((host) => host.lastConnected !== null)
      .sort((a, b) => Date.parse(b.lastConnected!) - Date.parse(a.lastConnected!));

    return {
      hosts: rows,
      groups,
      recent,
      onlineCount: rows.filter((host) => host.status === "online").length,
      loading,
      error,
      getHost: (id) => rows.find((host) => host.id === id),
      getConnection: (id) => connections[id],
      getHealth: (id) => health[id],
      refresh,
      save,
      remove,
      probe,
      connect,
      disconnect,
      power,
    };
  }, [
    hosts,
    connections,
    health,
    loading,
    error,
    refresh,
    save,
    remove,
    probe,
    connect,
    disconnect,
    power,
  ]);

  return <HostsContext.Provider value={value}>{children}</HostsContext.Provider>;
}

/**
 * Online means the machine answered, not that we are logged in.
 *
 * A host we have a session with is obviously online; so is one whose port
 * accepted a heartbeat. Only a host that was actually checked and failed
 * earns "offline" — claiming a machine is down when nobody looked is worse
 * than admitting we do not know.
 */
function statusFor(
  id: string,
  connections: Record<string, ConnectionInfo>,
  health: Record<string, HostHealth>,
): HostStatus {
  if (connections[id]) return "online";

  const last = health[id];
  if (!last) return "unknown";
  return last.reachable ? "online" : "offline";
}

export function useHosts(): HostsContextValue {
  const context = useContext(HostsContext);
  if (!context) {
    throw new Error("useHosts must be used inside a <HostsProvider>");
  }
  return context;
}
