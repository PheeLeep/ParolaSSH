import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useHosts } from "../hosts/HostsProvider";
import * as api from "./api";
import type { VpnBinding, VpnResource, VpnStatus } from "./types";

/** Same cadence as the host heartbeat: fresh enough to notice a VPN
 *  reconnect, cheap enough for the couple of local CLI calls it costs. */
const POLL_INTERVAL_MS = 30_000;

type VpnContextValue = {
  /** Every VPN the backend recognises, installed or not. */
  statuses: VpnStatus[];
  /** What `twingate resources` reported, when it could. */
  resources: VpnResource[];
  /** The VPN a saved address is reached through, if any is known. */
  bindingFor: (hostname: string) => VpnBinding | undefined;
  /** ISO 8601 of the last completed check, or null before the first. */
  lastChecked: string | null;
  /** Re-check now, ignoring the poll schedule. */
  refresh: () => Promise<void>;
};

const VpnContext = createContext<VpnContextValue | null>(null);

export function VpnProvider({ children }: { children: ReactNode }) {
  const { hosts } = useHosts();
  const [statuses, setStatuses] = useState<VpnStatus[]>([]);
  const [resources, setResources] = useState<VpnResource[]>([]);
  const [bindings, setBindings] = useState<Record<string, VpnBinding>>({});
  const [lastChecked, setLastChecked] = useState<string | null>(null);

  // Deduplicated and sorted so the poll effect re-runs only when the set of
  // addresses actually changes — not when a host is renamed or re-grouped.
  const hostnamesKey = useMemo(
    () => [...new Set(hosts.map((host) => host.hostname))].sort().join("\n"),
    [hosts],
  );

  const poll = useCallback(
    async (force = false) => {
      if (!force && document.hidden) return;
      const hostnames = hostnamesKey === "" ? [] : hostnamesKey.split("\n");
      try {
        // `force` means the user asked, in both senses: poll while hidden,
        // and go past the backend's cache to the clients themselves.
        const overview = await api.vpnOverview(hostnames, force);
        setStatuses(overview.statuses);
        setResources(overview.twingateResources);
        setBindings(
          Object.fromEntries(
            overview.bindings.map((binding) => [binding.hostname, binding]),
          ),
        );
        setLastChecked(new Date().toISOString());
      } catch {
        // Detection is best-effort decoration; a failed poll keeps the last
        // picture rather than flashing the indicator away.
      }
    },
    [hostnamesKey],
  );

  useEffect(() => {
    void poll();
    const timer = window.setInterval(() => void poll(), POLL_INTERVAL_MS);

    const onVisibility = () => {
      if (!document.hidden) void poll();
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [poll]);

  const value = useMemo<VpnContextValue>(
    () => ({
      statuses,
      resources,
      bindingFor: (hostname) => bindings[hostname],
      lastChecked,
      refresh: () => poll(true),
    }),
    [statuses, resources, bindings, lastChecked, poll],
  );

  return <VpnContext.Provider value={value}>{children}</VpnContext.Provider>;
}

export function useVpn(): VpnContextValue {
  const context = useContext(VpnContext);
  if (!context) {
    throw new Error("useVpn must be used inside a <VpnProvider>");
  }
  return context;
}
