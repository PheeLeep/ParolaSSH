

import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "./api";

const THEMES = {
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

export type TerminalEntry = {
  shellId: number;
  hostId: string;
  title: string;
  exited: boolean;
  exitCode: number | null;
  terminal: Terminal;
  fit: FitAddon;
  node: HTMLDivElement;
  unlisteners: UnlistenFn[];
};

const entries = new Map<number, TerminalEntry>();
const listeners = new Set<() => void>();


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


export async function open(
  hostId: string,
  theme: "light" | "dark",
  title?: string,
): Promise<number> {
  const terminal = new Terminal({
    fontFamily:
      'ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace',
    fontSize: 13,
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
      terminal.write("\r\n\x1b[90m— session ended —\x1b[0m\r\n");
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
    void api.writeShell(hostId, shellId!, data).catch(() => {
      
    });
  });

  entries.set(shellId, {
    shellId,
    hostId,
    title: title ?? `shell ${countForHost(hostId) + 1}`,
    exited: false,
    exitCode: null,
    terminal,
    fit,
    node,
    unlisteners,
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
  resize();

  return () => {
    observer.disconnect();
    entry.node.remove();
  };
}


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


export function rename(shellId: number, title: string) {
  const entry = entries.get(shellId);
  if (!entry) return;
  entry.title = title.trim() || entry.title;
  emit();
}

export function focus(shellId: number) {
  entries.get(shellId)?.terminal.focus();
}

export function clear(shellId: number) {
  entries.get(shellId)?.terminal.clear();
}
