

import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "./api";
import {
  clampFontSize,
  readTerminalFont,
  subscribeTerminalFont,
  type TerminalFont,
} from "../settings/preferences";

/** One palette for every piece of remote output in the app. */
export const THEMES = {
  dark: {
    background: "#12151c",
    foreground: "#d7dce5",
    cursor: "#7aa2f7",
    selectionBackground: "#2a3350",
  },
  light: {
    background: "#ffffff",
    foreground: "#1f2430",
    cursor: "#3355cc",
    selectionBackground: "#cfd8f5",
  },
};

const SCROLLBACK = 5000;

/** Longer than this and the tab strip is all one tab; the label ellipsises at
 *  11rem anyway, so the cap only stops absurd input reaching the store. */
export const MAX_TERMINAL_TITLE = 60;

export type TerminalEntry = {
  shellId: number;
  hostId: string;
  title: string;
  /** What `title` started as, so clearing a rename restores it rather than
   *  leaving the tab stuck with a name the user just tried to delete. */
  defaultTitle: string;
  exited: boolean;
  exitCode: number | null;
  terminal: Terminal;
  fit: FitAddon;
  node: HTMLDivElement;
  unlisteners: UnlistenFn[];
  /** Set per tab; whatever is absent follows the global default. */
  fontOverride: Partial<TerminalFont>;
  /** Refits against the mount, while this terminal is the attached one. */
  refit: (() => void) | null;
};

const entries = new Map<number, TerminalEntry>();
const listeners = new Set<() => void>();

// Broadcast: type once, send to every selected shell.
let broadcastTargets = new Set<string>();
let broadcastEnabled = false;

let version = 0;

function emit() {
  version += 1;
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getVersion(): number {
  return version;
}

export function get(shellId: number): TerminalEntry | undefined {
  return entries.get(shellId);
}


export function forHost(hostId: string): TerminalEntry[] {
  return [...entries.values()]
    .filter((entry) => entry.hostId === hostId)
    .sort((a, b) => a.shellId - b.shellId);
}

export function countForHost(hostId: string): number {
  return forHost(hostId).length;
}

/** Every open terminal, across hosts - what the Sessions view lists. */
export function all(): TerminalEntry[] {
  return [...entries.values()].sort((a, b) => a.shellId - b.shellId);
}

/** Shells still running remotely. An exited tab stays until it is closed,
 *  and counting those would report sessions we no longer hold. */
export function liveCount(): number {
  let count = 0;
  for (const entry of entries.values()) {
    if (!entry.exited) count += 1;
  }
  return count;
}


export async function open(
  hostId: string,
  theme: "light" | "dark",
  title?: string,
): Promise<number> {
  const font = readTerminalFont();
  const terminal = new Terminal({
    fontFamily: font.family,
    fontSize: font.size,
    lineHeight: 1.2,
    cursorBlink: true,
    scrollback: SCROLLBACK,
    allowProposedApi: true,
    theme: THEMES[theme],
  });

  const fit = new FitAddon();
  terminal.loadAddon(fit);

  const node = document.createElement("div");
  node.className = "terminal-host";
  terminal.open(node);

  const unlisteners: UnlistenFn[] = [];
  let shellId: number | null = null;

  const isMine = (candidate: number) => shellId === candidate;

  unlisteners.push(
    await api.onTerminalOutput(hostId, isMine, ({ chunk }) => {
      terminal.write(chunk);
    }),
  );
  unlisteners.push(
    await api.onTerminalClosed(hostId, isMine, ({ exitCode }) => {
      const entry = shellId === null ? undefined : entries.get(shellId);
      if (!entry || entry.exited) return;
      entry.exited = true;
      entry.exitCode = exitCode;
      terminal.write("\r\n\x1b[90m- session ended -\x1b[0m\r\n");
      emit();
    }),
  );

  try {
    
    shellId = await api.openShell(hostId, terminal.cols || 80, terminal.rows || 24);
  } catch (error) {
    // Nothing was created remotely, so tear down everything local.
    for (const unlisten of unlisteners) unlisten();
    terminal.dispose();
    node.remove();
    throw error;
  }

  terminal.onData((data) => {
    if (broadcastEnabled && broadcastTargets.has(broadcastKey(hostId, shellId!))) {
      const targets = [...broadcastTargets].map((key) => {
        const [h, s] = key.split(":");
        return { hostId: h, shellId: Number(s) };
      });
      void api.broadcastShells(targets, data).catch(() => {});
    } else {
      void api.writeShell(hostId, shellId!, data).catch(() => {});
    }
  });

  const defaultTitle = title ?? `shell ${countForHost(hostId) + 1}`;

  entries.set(shellId, {
    shellId,
    hostId,
    title: defaultTitle,
    defaultTitle,
    exited: false,
    exitCode: null,
    terminal,
    fit,
    node,
    unlisteners,
    fontOverride: {},
    refit: null,
  });

  emit();
  return shellId;
}


export function attach(shellId: number, mount: HTMLElement): () => void {
  const entry = entries.get(shellId);
  if (!entry) return () => undefined;

  mount.appendChild(entry.node);

  const resize = () => {
    try {
      entry.fit.fit();
      void api
        .resizeShell(entry.hostId, entry.shellId, entry.terminal.cols, entry.terminal.rows)
        .catch(() => undefined);
    } catch {
    }
  };

  
  const observer = new ResizeObserver(resize);
  observer.observe(mount);
  entry.refit = resize;
  resize();

  return () => {
    observer.disconnect();
    entry.refit = null;
    entry.node.remove();
  };
}

/** What this tab actually renders with: its own overrides over the global. */
export function fontFor(shellId: number): TerminalFont {
  const global = readTerminalFont();
  const entry = entries.get(shellId);
  return { ...global, ...entry?.fontOverride };
}

export function hasFontOverride(shellId: number): boolean {
  const entry = entries.get(shellId);
  return Boolean(entry && Object.keys(entry.fontOverride).length > 0);
}

function applyFont(entry: TerminalEntry) {
  const { family, size } = { ...readTerminalFont(), ...entry.fontOverride };
  entry.terminal.options.fontFamily = family;
  entry.terminal.options.fontSize = size;
  // Cell size changed, so the row/column count did too and the remote pty has
  // to be told, or output wraps against the old geometry. A frame late,
  // because xterm re-measures the cell before it can be fitted against.
  requestAnimationFrame(() => entry.refit?.());
}

/** `patch` of null drops the override and puts the tab back on the global. */
export function setFontOverride(
  shellId: number,
  patch: Partial<TerminalFont> | null,
): void {
  const entry = entries.get(shellId);
  if (!entry) return;

  entry.fontOverride = patch
    ? {
        ...entry.fontOverride,
        ...patch,
        ...(patch.size === undefined ? {} : { size: clampFontSize(patch.size) }),
      }
    : {};

  applyFont(entry);
  emit();
}

// Open terminals follow the global default. Re-applying is safe for the ones
// holding an override too: only the fields they did not set move, so a tab
// that pinned its size still picks up a new family.
subscribeTerminalFont(() => {
  for (const entry of entries.values()) applyFont(entry);
  emit();
});


export function applyTheme(theme: "light" | "dark") {
  for (const entry of entries.values()) {
    entry.terminal.options.theme = THEMES[theme];
  }
}


export async function close(shellId: number): Promise<void> {
  const entry = entries.get(shellId);
  if (!entry) return;

  entries.delete(shellId);
  emit();

  for (const unlisten of entry.unlisteners) unlisten();
  entry.unlisteners.length = 0;

  entry.terminal.dispose();
  entry.node.remove();

  await api.closeShell(entry.hostId, shellId).catch(() => undefined);
}


export async function closeHost(hostId: string): Promise<void> {
  await Promise.all(forHost(hostId).map((entry) => close(entry.shellId)));
}


/** Rename a tab. An empty name restores the one it was opened with. */
export function rename(shellId: number, title: string) {
  const entry = entries.get(shellId);
  if (!entry) return;

  const trimmed = title.trim().slice(0, MAX_TERMINAL_TITLE);
  const next = trimmed || entry.defaultTitle;
  if (next === entry.title) return;

  entry.title = next;
  emit();
}

export function focus(shellId: number) {
  entries.get(shellId)?.terminal.focus();
}

export function clear(shellId: number) {
  entries.get(shellId)?.terminal.clear();
}

/* ── Broadcast ──────────────────────────────────────────────────────────── */

function broadcastKey(hostId: string, shellId: number): string {
  return `${hostId}:${shellId}`;
}

export function setBroadcastTargets(targets: Set<string>) {
  broadcastTargets = targets;
  broadcastEnabled = targets.size > 0;
  emit();
}

export function clearBroadcast() {
  broadcastTargets = new Set();
  broadcastEnabled = false;
  emit();
}

export function isBroadcasting(): boolean {
  return broadcastEnabled;
}

export function getBroadcastTargets(): Set<string> {
  return broadcastTargets;
}

export function isBroadcastTarget(hostId: string, shellId: number): boolean {
  return broadcastTargets.has(broadcastKey(hostId, shellId));
}
