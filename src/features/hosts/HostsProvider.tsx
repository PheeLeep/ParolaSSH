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
import * as auditCache from "./auditCache";
import { readAutoAudit } from "../settings/preferences";
import * as terminals from "./terminalStore";
import * as tasks from "./taskStore";
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

const HEARTBEAT_INTERVAL_MS = 30_000;

export type HostRow = SshHost & { status: HostStatus };

export type HostGroup = {
  name: string;
  hosts: HostRow[];
  connectedCount: number;
};

type ConnectOptions = {
  password?: string | null;
  remember?: boolean;
  trustUnknown?: boolean;
};

type HostsContextValue = {
  hosts: HostRow[];
  groups: HostGroup[];
  recent: HostRow[];
  connectedCount: number;
  loading: boolean;
  error: string | null;

  getHost: (id: string) => HostRow | undefined;
  getConnection: (id: string) => ConnectionInfo | undefined;
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
  const [health, setHealth] = useState<Record<string, HostHealth>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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

  const connectionsRef = useRef(connections);
  useEffect(() => {
    connectionsRef.current = connections;
  }, [connections]);

  const remove = useCallback(
    async (id: string) => {
      if (connectionsRef.current[id]) {
        await terminals.closeHost(id).catch(() => undefined);
        await tasks.closeHost(id).catch(() => undefined);
        auditCache.forget(id);
        await api.disconnectHost(id).catch(() => undefined);
      }
      // Tasks pinned to this host go with it: the file must not
      // accumulate commands aimed at a machine that is gone.
      await api.forgetHostTasks(id).catch(() => undefined);
      await api.deleteHost(id);
      setConnections(({ [id]: _removed, ...rest }) => rest);
      await refresh();
    },
    [refresh],
  );

  const probe = useCallback(async (hostname: string, port: number) => {
    return api.probeHost(hostname, port);
  }, []);

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

        const stillConnected = new Set(
          results.filter((entry) => entry.connected).map((entry) => entry.hostId),
        );
        setConnections((previous) => {
          const next: Record<string, ConnectionInfo> = {};
          for (const [hostId, info] of Object.entries(previous)) {
            if (stillConnected.has(hostId)) next[hostId] = info;
            else {
              void terminals.closeHost(hostId);
              void tasks.closeHost(hostId);
              auditCache.forget(hostId);
            }
          }
          return Object.keys(next).length === Object.keys(previous).length
            ? previous
            : next;
        });
      } catch {

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
      void autoAudit(id);
      return info;
    } catch (caught) {
      setHealth((previous) => ({
        ...previous,
        [id]: { hostId: id, connected: false, reachable: false, latencyMs: null },
      }));
      throw caught;
    }
  }, []);

  const disconnect = useCallback(async (id: string) => {
    await terminals.closeHost(id);
    await tasks.closeHost(id);
    auditCache.forget(id);
    await api.disconnectHost(id);
    setConnections(({ [id]: _removed, ...rest }) => rest);

    // The mirror of the note in `statusFor`: dropping the connection is not
    // enough, because the last heartbeat still says `connected` and that is
    // the other half of the same test. Without this the chain stays whole for
    // up to 30 s after the session is gone.
    //
    // Only the session ended — the machine itself is as reachable as it was a
    // moment ago, so that half of the answer is carried over rather than
    // reset, which would wrongly paint the host offline.
    setHealth((previous) => {
      const last = previous[id];
      return {
        ...previous,
        [id]: {
          hostId: id,
          connected: false,
          reachable: last?.reachable ?? true,
          latencyMs: last?.latencyMs ?? null,
        },
      };
    });
  }, []);

  const power = useCallback(
    async (id: string, request: PowerRequest, password?: string | null) => {
      const outcome = await api.powerHost(id, request, password);

      const terminal =
        outcome.succeeded && request.action !== "cancel" && request.delayMinutes === 0;
      if (terminal) {
        void terminals.closeHost(id);
        void tasks.closeHost(id);
        auditCache.forget(id);
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
        connectedCount: groupHosts.filter((host) => host.status === "connected").length,
      }))
      .sort((a, b) => a.name.localeCompare(b.name));

    const recent = rows
      .filter((host) => host.lastConnected !== null)
      .sort((a, b) => Date.parse(b.lastConnected!) - Date.parse(a.lastConnected!));

    return {
      hosts: rows,
      groups,
      recent,
      connectedCount: rows.filter((host) => host.status === "connected").length,
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
 * Settings › Startup › "Check posture on connect", honoured at the moment of
 * connecting rather than when the Audit pane is opened — a check that waits for
 * a pane to be opened is not "on connect", and the pane may never be opened.
 *
 * Unprivileged always (`elevate: false`). A sudo prompt raised by connecting is
 * a prompt nobody asked for, and a password sent automatically is not consent;
 * the report names the checks it had to skip, exactly as it does when
 * elevation is declined by hand.
 *
 * Failure is silent by design. This is a background courtesy, not something the
 * operator asked for right now, and a toast about it would interrupt whatever
 * they connected to do. Opening the pane and pressing the button reports
 * properly.
 */
async function autoAudit(hostId: string): Promise<void> {
  if (!readAutoAudit()) return;
  if (!auditCache.markAttempted(hostId)) return;

  try {
    auditCache.set(hostId, await api.remoteAudit(hostId, null, false));
  } catch {
    // Silent: see above.
  }
}

function statusFor(
  id: string,
  connections: Record<string, ConnectionInfo>,
  health: Record<string, HostHealth>,
): HostStatus {
  // `connections` first: right after connect() the health entry is stale
  // until the next heartbeat, and would read "reachable" for up to 30 s.
  if (connections[id] || health[id]?.connected) return "connected";

  const last = health[id];
  if (!last) return "unknown";
  return last.reachable ? "reachable" : "offline";
}

export function useHosts(): HostsContextValue {
  const context = useContext(HostsContext);
  if (!context) {
    throw new Error("useHosts must be used inside a <HostsProvider>");
  }
  return context;
}
