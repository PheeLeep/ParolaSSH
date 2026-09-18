/**
 * The app's own update state, shared by the banner and Settings.
 *
 * Nothing installs without a click. The updater verifies each download against
 * the public key in tauri.conf.json before running it.
 */

import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export const RELEASES_URL = "https://github.com/PheeLeep/ParolaSSH/releases/latest";

/** Whether this copy can replace itself; a .deb belongs to apt. */
export type InstallKind = "updatable" | "package" | "unsupported";

export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "current"; checkedAt: number }
  | { status: "available"; version: string; notes: string | null; kind: InstallKind }
  | { status: "installing"; version: string; percent: number | null }
  | { status: "error"; message: string };

let state: UpdateState = { status: "idle" };
let pending: Update | null = null;
let dismissed: string | null = null;
const listeners = new Set<() => void>();

function set(next: UpdateState) {
  state = next;
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const getState = (): UpdateState => state;

/** Whether the banner should show: an update the user has not waved off. */
export function bannerVisible(): boolean {
  if (state.status === "installing") return true;
  return state.status === "available" && state.version !== dismissed;
}

export function dismiss(): void {
  if (state.status === "available") dismissed = state.version;
  set(state);
}

function message(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

/** Ask GitHub for a newer release. `quiet` keeps a failed startup check silent. */
export async function checkForUpdate({ quiet = false } = {}): Promise<void> {
  if (state.status === "checking" || state.status === "installing") return;
  const kind = await invoke<InstallKind>("install_kind").catch(() => "unsupported" as const);
  if (kind === "unsupported") {
    if (!quiet) set({ status: "error", message: "Development builds do not update themselves." });
    return;
  }

  set({ status: "checking" });
  try {
    const update = await check();
    pending = update;
    set(
      update
        ? { status: "available", version: update.version, notes: update.body ?? null, kind }
        : { status: "current", checkedAt: Date.now() },
    );
  } catch (caught) {
    set(quiet ? { status: "idle" } : { status: "error", message: message(caught) });
  }
}

/** Download, verify and install, then restart into the new version. */
export async function installUpdate(): Promise<void> {
  if (state.status !== "available" || !pending) return;
  const update = pending;
  const version = state.version;
  let total = 0;
  let received = 0;

  set({ status: "installing", version, percent: null });
  try {
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      if (event.event === "Progress") {
        received += event.data.chunkLength;
        if (total > 0) set({ status: "installing", version, percent: Math.round((received / total) * 100) });
      }
    });
    // Windows' installer has already closed the app by now; this is Linux.
    await relaunch();
  } catch (caught) {
    set({ status: "error", message: `The update could not be installed: ${message(caught)}` });
  }
}

/** Test hook: back to a fresh launch. */
export function reset(): void {
  state = { status: "idle" };
  pending = null;
  dismissed = null;
}
