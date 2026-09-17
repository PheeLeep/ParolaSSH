import type { ConnectionInfo, HostHealth, HostStatus } from "./types";

/** The status dot: live session, reachable, unreachable, or never probed. */
export function statusFor(
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
