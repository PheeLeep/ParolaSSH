import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { sampleHosts } from "./sampleHosts";
import type { SshHost } from "./types";

export type HostGroup = {
  name: string;
  hosts: SshHost[];
  onlineCount: number;
};

type HostsContextValue = {
  hosts: SshHost[];
  groups: HostGroup[];
  /** Most recently connected first; never-connected hosts excluded. */
  recent: SshHost[];
  onlineCount: number;
  getHost: (id: string) => SshHost | undefined;
  connect: (host: SshHost) => void;
  edit: (host: SshHost) => void;
  remove: (host: SshHost) => void;
};

const HostsContext = createContext<HostsContextValue | null>(null);

export function HostsProvider({ children }: { children: ReactNode }) {
  // TODO: load from the Rust side once connections are persisted.
  const [hosts] = useState<SshHost[]>(sampleHosts);

  const connect = useCallback((host: SshHost) => {
    console.info("connect", host.id);
  }, []);
  const edit = useCallback((host: SshHost) => {
    console.info("edit", host.id);
  }, []);
  const remove = useCallback((host: SshHost) => {
    console.info("delete", host.id);
  }, []);

  const value = useMemo<HostsContextValue>(() => {
    const byGroup = new Map<string, SshHost[]>();
    for (const host of hosts) {
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

    const recent = hosts
      .filter((host) => host.lastConnected !== null)
      .sort(
        (a, b) => Date.parse(b.lastConnected!) - Date.parse(a.lastConnected!),
      );

    return {
      hosts,
      groups,
      recent,
      onlineCount: hosts.filter((host) => host.status === "online").length,
      getHost: (id) => hosts.find((host) => host.id === id),
      connect,
      edit,
      remove,
    };
  }, [hosts, connect, edit, remove]);

  return <HostsContext.Provider value={value}>{children}</HostsContext.Provider>;
}

export function useHosts(): HostsContextValue {
  const context = useContext(HostsContext);
  if (!context) {
    throw new Error("useHosts must be used inside a <HostsProvider>");
  }
  return context;
}
