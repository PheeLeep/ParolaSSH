/** Task runs, owned outside React's tree.
 *
 *  A run belongs to the host, not to the pane that started it. Switching
 *  hosts, switching tabs and navigating away close nothing — a four-minute
 *  backup keeps going and keeps collecting output while you look at something
 *  else. Runs end on the four moments a terminal does: host disconnected,
 *  heartbeat reaped it, the user stopped it, app exit. `closeHost` is called
 *  from all four sites in `HostsProvider`.
 *
 *  The xterm is the same reason the terminals use one: real commands emit
 *  cursor and colour escapes, and a `<pre>` shows them as literal text. The
 *  palette is imported rather than copied, so remote output looks like remote
 *  output everywhere in the app.
 */

import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "./api";
import { THEMES } from "./terminalStore";
import { readTerminalFont } from "../settings/preferences";
import type { TaskPlan } from "./types";

const SCROLLBACK = 5000;

/** Lynis taught this: a run's output arrives every 16 ms, and a store that
 *  notified on each chunk re-rendered the card at that rate to say the same
 *  thing. Only a *changed* summary notifies. */
export type RunState = "running" | "finished" | "failed" | "stopped";

export type TaskRun = {
  hostId: string;
  taskId: string;
  taskName: string;
  /** The plan as approved — what the pane shows while it runs. */
  plan: TaskPlan;
  state: RunState;
  startedAt: number;
  finishedAt: number | null;
  exitCode: number | null;
  /** Set when the run could not start at all, or ended badly. */
  error: string | null;
  streamId: number | null;
  terminal: Terminal;
  fit: FitAddon;
  node: HTMLDivElement;
  unlisteners: UnlistenFn[];
  /** Refits against the mount while this run is the attached one. */
  refit: (() => void) | null;
};

/** One run per host. Starting a second on the same host is refused rather
 *  than queued: two tasks writing to one machine at once is a decision, not a
 *  default, and the first press must not be silently replaced by the second. */
const runs = new Map<string, TaskRun>();
const listeners = new Set<() => void>();
let version = 0;

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function notify() {
  version += 1;
  for (const listener of listeners) listener();
}

export function getVersion(): number {
  return version;
}

export function get(hostId: string): TaskRun | undefined {
  return runs.get(hostId);
}

export function isRunning(hostId: string): boolean {
  return runs.get(hostId)?.state === "running";
}

function makeTerminal(theme: "light" | "dark"): {
  terminal: Terminal;
  fit: FitAddon;
  node: HTMLDivElement;
} {
  const node = document.createElement("div");
  node.className = "terminal-host";

  const font = readTerminalFont();

  const terminal = new Terminal({
    // The stream carries bare `\n`; without this every line would start where
    // the last one ended.
    convertEol: true,
    // Nothing is ever typed at a task. There is no keystroke path from this
    // pane to the command it is running.
    disableStdin: true,
    cursorBlink: false,
    cursorStyle: "underline",
    scrollback: SCROLLBACK,
    fontFamily: font.family,
    fontSize: font.size,
    theme: THEMES[theme],
  });

  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(node);

  return { terminal, fit, node };
}

/** Start a task. Throws rather than returning quietly when one is already in
 *  flight on this host — a button that does nothing and says nothing is
 *  indistinguishable from a broken one. */
export async function start(
  hostId: string,
  taskId: string,
  taskName: string,
  plan: TaskPlan,
  theme: "light" | "dark",
  password?: string | null,
): Promise<void> {
  const existing = runs.get(hostId);
  if (existing?.state === "running") {
    throw new Error(
      `“${existing.taskName}” is still running on this host. Wait for it, or stop it first.`,
    );
  }
  // A finished run is replaced by the next one; its output has been on screen
  // and its terminal is disposed here rather than leaking.
  if (existing) disposeRun(existing);

  const { terminal, fit, node } = makeTerminal(theme);

  const run: TaskRun = {
    hostId,
    taskId,
    taskName,
    plan,
    state: "running",
    startedAt: Date.now(),
    finishedAt: null,
    exitCode: null,
    error: null,
    streamId: null,
    terminal,
    fit,
    node,
    unlisteners: [],
    refit: null,
  };
  runs.set(hostId, run);
  notify();

  // The command is echoed into the feed before anything runs, so the log is
  // self-describing: a transcript that does not say what it ran is evidence
  // of nothing.
  terminal.writeln(`\x1b[2m$ ${plan.command}\x1b[0m`);

  // Listeners go on *before* the command is asked for. The host starts writing
  // the moment the channel opens, and a task short enough to finish inside one
  // round trip would otherwise lose its output to a listener that was still
  // being attached — which is most of the built-ins.
  //
  // That leaves a smaller window: events can arrive before this side learns
  // which stream id is its own. They are buffered by id and flushed once it is
  // known, rather than matched loosely — a followed journal on the same host is
  // also emitting, and taking its output would be worse than dropping ours.
  const pending = new Map<number, string[]>();
  // Two numbers rather than a nullable record: the assignment happens inside a
  // callback, and TypeScript would narrow a `T | null` closure variable to
  // `null` at the point this is read back.
  let closedStreamId = -1;
  let closedExitCode: number | null = null;

  const mine = (id: number) => run.streamId !== null && run.streamId === id;

  run.unlisteners.push(
    await api.onStreamOutput(
      hostId,
      () => true,
      ({ streamId, chunk }) => {
        if (mine(streamId)) {
          run.terminal.write(chunk);
        } else if (run.streamId === null) {
          const buffered = pending.get(streamId) ?? [];
          buffered.push(chunk);
          pending.set(streamId, buffered);
        }
      },
    ),
    await api.onStreamClosed(
      hostId,
      () => true,
      ({ streamId, exitCode }) => {
        if (mine(streamId)) {
          settle(hostId, exitCode === 0 ? "finished" : "failed", exitCode, null);
        } else if (run.streamId === null) {
          closedStreamId = streamId;
          closedExitCode = exitCode;
        }
      },
    ),
  );

  try {
    const streamId = await api.startTask(hostId, taskId, plan.elevated, password);
    run.streamId = streamId;

    for (const chunk of pending.get(streamId) ?? []) run.terminal.write(chunk);
    pending.clear();

    // A task that finished before its id came back is finished, not running.
    if (closedStreamId === streamId) {
      settle(
        hostId,
        closedExitCode === 0 ? "finished" : "failed",
        closedExitCode,
        null,
      );
    }
  } catch (caught) {
    const message = caught instanceof Error ? caught.message : String(caught);
    run.terminal.writeln(`\r\n\x1b[31m${message}\x1b[0m`);
    settle(hostId, "failed", null, message);
    throw caught;
  }

  notify();
}

/** Stop watching. This closes *our* channel — it does not reach in and kill a
 *  process on the host, and the pane says so rather than implying otherwise. */
export async function stop(hostId: string): Promise<void> {
  const run = runs.get(hostId);
  if (!run || run.state !== "running") return;

  const streamId = run.streamId;
  // Settled first: closing the stream fires `stream://closed`, and clearing
  // the id before that event lands would make it unroutable.
  settle(hostId, "stopped", null, null);

  if (streamId !== null) {
    await api.closeStream(hostId, streamId).catch(() => undefined);
  }
}

function settle(
  hostId: string,
  state: RunState,
  exitCode: number | null,
  error: string | null,
) {
  const run = runs.get(hostId);
  if (!run || run.state !== "running") return;

  run.state = state;
  run.exitCode = exitCode;
  run.error = error;
  run.finishedAt = Date.now();

  for (const unlisten of run.unlisteners) unlisten();
  run.unlisteners = [];

  if (state === "stopped") {
    run.terminal.writeln(
      "\r\n\x1b[33mStopped watching. If the command was still running on the host, " +
        "it keeps running there — closing this channel does not kill it.\x1b[0m",
    );
  }

  notify();
}

/** Attach the run's terminal into a mount, and keep it fitted. The terminal
 *  lives outside React, so scrollback survives closing the view and output
 *  keeps arriving while it is hidden. */
export function attach(hostId: string, mount: HTMLElement): () => void {
  const run = runs.get(hostId);
  if (!run) return () => undefined;

  mount.appendChild(run.node);

  const refit = () => {
    try {
      run.fit.fit();
    } catch {
      // A mount with no layout yet — the next resize does it.
    }
  };
  run.refit = refit;
  refit();

  const observer = new ResizeObserver(refit);
  observer.observe(mount);

  return () => {
    observer.disconnect();
    if (run.refit === refit) run.refit = null;
    if (run.node.parentElement === mount) mount.removeChild(run.node);
  };
}

function disposeRun(run: TaskRun) {
  for (const unlisten of run.unlisteners) unlisten();
  run.unlisteners = [];
  run.terminal.dispose();
  run.node.remove();
}

/** Everything this host had. Called on disconnect, reap, delete and app exit. */
export async function closeHost(hostId: string): Promise<void> {
  const run = runs.get(hostId);
  if (!run) return;

  if (run.state === "running" && run.streamId !== null) {
    await api.closeStream(hostId, run.streamId).catch(() => undefined);
  }
  disposeRun(run);
  runs.delete(hostId);
  notify();
}

/** Follow the app's theme, like the terminals do. */
export function applyTheme(theme: "light" | "dark"): void {
  for (const run of runs.values()) {
    run.terminal.options.theme = THEMES[theme];
  }
}

/** Drop a finished run's output without touching a live one. */
export function clear(hostId: string): void {
  const run = runs.get(hostId);
  if (!run || run.state === "running") return;

  disposeRun(run);
  runs.delete(hostId);
  notify();
}
