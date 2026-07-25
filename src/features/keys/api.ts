/** The Rust command surface.
 *
 *  These are the only filesystem verbs the frontend has — there is no fs
 *  plugin scoped to ~/.ssh, deliberately, so nothing here can return private
 *  key material. */

import { invoke } from "@tauri-apps/api/core";
import type {
  AuditReport,
  GenerateOutcome,
  GenerateRequest,
  KeyPermissions,
  KeyScan,
  SshLocation,
} from "./types";

export const sshLocation = () => invoke<SshLocation>("ssh_location");

export const listSshKeys = () => invoke<KeyScan>("list_ssh_keys");

export const auditSshDir = () => invoke<AuditReport>("audit_ssh_dir");

/** Dismiss or restore a finding; returns the refreshed report. */
export const setFindingSuppressed = (findingId: string, suppressed: boolean) =>
  invoke<AuditReport>("set_finding_suppressed", { findingId, suppressed });

export const restrictPermissions = (path: string, isDir: boolean) =>
  invoke<KeyPermissions>("restrict_permissions", { path, isDir });

export const generateSshKey = (request: GenerateRequest) =>
  invoke<GenerateOutcome>("generate_ssh_key", { request });

/** Permanently deletes the key. Returns the paths that were removed. */
export const deleteSshKey = (path: string, includePublic: boolean) =>
  invoke<string[]>("delete_ssh_key", { path, includePublic });

export const verifyKeyPassphrase = (path: string, passphrase: string) =>
  invoke<boolean>("verify_key_passphrase", { path, passphrase });

export const readPublicKey = (path: string) =>
  invoke<string>("read_public_key", { path });

/** Rust errors arrive as plain strings; anything else is a bug worth showing. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
