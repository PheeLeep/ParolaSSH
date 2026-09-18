import type { ConnectionInfo, Platform } from "../features/hosts/types";

export const fullSystem: Platform = { init: "systemd", container: null, pid1: "systemd", hasShutdown: true };

/** A live connection as the providers expose it; override what the test is about. */
export function connectionInfo(overrides: Partial<ConnectionInfo> = {}): ConnectionInfo {
  return {
    hostId: "h1",
    connected: true,
    os: "linux",
    osDetail: "Linux",
    user: "root",
    elevation: { kind: "notNeeded" },
    elevationExplanation: "You are connected as root.",
    supportsForce: false,
    supportsCancel: true,
    supportsDelay: true,
    powerRefusal: null,
    platform: fullSystem,
    fingerprint: null,
    negotiated: null,
    connectedAt: "2026-09-18T08:00:00Z",
    shellIds: [],
    hasLoginPassword: false,
    ...overrides,
  };
}
