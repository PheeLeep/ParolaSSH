export type AuthMethod = "password" | "publickey" | "agent";

export type HostStatus = "online" | "offline" | "unknown";

export interface SshHost {
  id: string;
  /** Friendly name shown in the list. */
  label: string;
  hostname: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  /** Folder / environment this host belongs to. */
  group: string;
  tags: string[];
  /** ISO 8601, or null if never connected. */
  lastConnected: string | null;
  status: HostStatus;
}

export const AUTH_METHOD_LABELS: Record<AuthMethod, string> = {
  password: "Password",
  publickey: "Public key",
  agent: "SSH agent",
};

export const STATUS_LABELS: Record<HostStatus, string> = {
  online: "Online",
  offline: "Offline",
  unknown: "Unknown",
};
