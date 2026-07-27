import { invoke } from "@tauri-apps/api/core";

export type LogLevel = "debug" | "info" | "warn" | "error";

export interface LogEntry {
  time: string;
  level: LogLevel;
  /** Which part of the app spoke: `ssh`, `transfers`, `app`… */
  target: string;
  message: string;
}

export interface LogLocation {
  path: string;
  exists: boolean;
  bytes: number;
}

/** Null when the platform gave us nowhere to write. */
export const appLogLocation = () =>
  invoke<LogLocation | null>("app_log_location");

/** The tail of the log, newest last. Rust caps this regardless of the ask. */
export const readAppLog = (maxLines?: number) =>
  invoke<LogEntry[]>("read_app_log", { maxLines });

export const clearAppLog = () => invoke<void>("clear_app_log");
