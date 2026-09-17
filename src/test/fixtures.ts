import type { HostRow } from "../features/hosts/HostsProvider";

/** A saved host as the providers expose it; override what the test is about. */
export function hostRow(overrides: Partial<HostRow> = {}): HostRow {
  return {
    id: "h1",
    label: "web-1",
    hostname: "192.168.56.10",
    port: 22,
    username: "pheeleep",
    authMethod: "password",
    keyPath: null,
    group: "Default",
    tags: [],
    notes: null,
    proxyJump: null,
    lastConnected: null,
    status: "unknown",
    ...overrides,
  };
}
