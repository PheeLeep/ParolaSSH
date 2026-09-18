/** Task runs, owned outside React's tree.
 *
 *  A run belongs to the host, not to the pane that started it. Switching
 *  hosts, switching tabs and navigating away close nothing - a four-minute
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
import { bindTerminalClipboard } from "../../lib/terminalClipboard";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { readTaskBlocking } from "../settings/preferences";
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
  /** The plan as approved - what the pane shows while it runs. */
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

/** Each host's runs, one tab per task. A task has at most one tab: running it
 *  again reruns it there rather than opening a second copy. */
type HostRuns = { runs: TaskRun[]; active: string | null };

/** Same ceiling as terminal tabs. */
export const MAX_TABS = 8;

const hosts = new Map<string, HostRuns>();
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

/** This host's tabs in order, and which one is showing. */
export function list(hostId: string): { runs: TaskRun[]; active: TaskRun | undefined } {
  const entry = hosts.get(hostId);
  if (!entry) return { runs: [], active: undefined };
  return { runs: entry.runs, active: entry.runs.find((run) => run.taskId === entry.active) };
}

export function find(hostId: string, taskId: string): TaskRun | undefined {
  return hosts.get(hostId)?.runs.find((run) => run.taskId === taskId);
}

export function select(hostId: string, taskId: string): void {
  const entry = hosts.get(hostId);
  if (!entry || entry.active === taskId || !find(hostId, taskId)) return;
  entry.active = taskId;
  notify();
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
  bindTerminalClipboard(terminal);
  terminal.open(node);

  return { terminal, fit, node };
}

/** Start a task in its tab, reusing the tab when it already has one. Throws
 *  when that task is still running or every tab is taken - a button that does
 *  nothing and says nothing is indistinguishable from a broken one. */
export async function start(
  hostId: string,
  taskId: string,
  taskName: string,
  plan: TaskPlan,
  theme: "light" | "dark",
  password?: string | null,
): Promise<void> {
  const entry = hosts.get(hostId) ?? { runs: [], active: null };
  const index = entry.runs.findIndex((run) => run.taskId === taskId);
  const existing = index >= 0 ? entry.runs[index] : undefined;

  if (existing?.state === "running") {
    entry.active = taskId;
    notify();
    throw new Error(`“${existing.taskName}” is still running. Wait for it, or stop it first.`);
  }
  if (!existing && entry.runs.length >= MAX_TABS) {
    throw new Error(`${MAX_TABS} task tabs are open. Close a finished one first.`);
  }

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

  // A rerun takes the old run's place in the strip; its output has been seen.
  if (existing) {
    disposeRun(existing);
    entry.runs[index] = run;
  } else {
    entry.runs.push(run);
  }
  entry.active = taskId;
  hosts.set(hostId, entry);
  notify();

  // The command is echoed into the feed before anything runs, so the log is
  // self-describing: a transcript that does not say what it ran is evidence
  // of nothing.
  // A PowerShell task is echoed as written; its encoded form says nothing.
  const shown = plan.wrapper === "powershell" ? `PS> ${plan.innerCommand}` : `$ ${plan.command}`;
  terminal.writeln(`\x1b[2m${shown}\x1b[0m`);

  // Listeners go on *before* the command is asked for. The host starts writing
  // the moment the channel opens, and a task short enough to finish inside one
  // round trip would otherwise lose its output to a listener that was still
  // being attached - which is most of the built-ins.
  //
  // That leaves a smaller window: events can arrive before this side learns
  // which stream id is its own. They are buffered by id and flushed once it is
  // known, rather than matched loosely - a followed journal on the same host is
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
          settle(run, exitCode === 0 ? "finished" : "failed", exitCode, null);
        } else if (run.streamId === null) {
          closedStreamId = streamId;
          closedExitCode = exitCode;
        }
      },
    ),
  );

  try {
    const streamId = await api.startTask(hostId, taskId, plan.elevated, password, readTaskBlocking());
    run.streamId = streamId;

    for (const chunk of pending.get(streamId) ?? []) run.terminal.write(chunk);
    pending.clear();

    // A task that finished before its id came back is finished, not running.
    if (closedStreamId === streamId) {
      settle(
        run,
        closedExitCode === 0 ? "finished" : "failed",
        closedExitCode,
        null,
      );
    }
  } catch (caught) {
    const message = caught instanceof Error ? caught.message : String(caught);
    run.terminal.writeln(`\r\n\x1b[31m${message}\x1b[0m`);
    settle(run, "failed", null, message);
    throw caught;
  }

  notify();
}

/** Stop watching. This closes *our* channel - it does not reach in and kill a
 *  process on the host, and the pane says so rather than implying otherwise. */
export async function stop(hostId: string, taskId: string): Promise<void> {
  const run = find(hostId, taskId);
  if (!run || run.state !== "running") return;

  const streamId = run.streamId;
  // Settled first: closing the stream fires `stream://closed`, and clearing
  // the id before that event lands would make it unroutable.
  settle(run, "stopped", null, null);

  if (streamId !== null) {
    await api.closeStream(hostId, streamId).catch(() => undefined);
  }
}

function settle(
  run: TaskRun,
  state: RunState,
  exitCode: number | null,
  error: string | null,
) {
  if (run.state !== "running") return;

  run.state = state;
  run.exitCode = exitCode;
  run.error = error;
  run.finishedAt = Date.now();

  for (const unlisten of run.unlisteners) unlisten();
  run.unlisteners = [];

  if (state === "stopped") {
    run.terminal.writeln(
      "\r\n\x1b[33mStopped watching. If the command was still running on the host, " +
        "it keeps running there - closing this channel does not kill it.\x1b[0m",
    );
  }

  notify();
}

/** Attach the run's terminal into a mount, and keep it fitted. The terminal
 *  lives outside React, so scrollback survives closing the view and output
 *  keeps arriving while it is hidden. */
export function attach(hostId: string, taskId: string, mount: HTMLElement): () => void {
  const run = find(hostId, taskId);
  if (!run) return () => undefined;

  mount.appendChild(run.node);

  const refit = () => {
    try {
      run.fit.fit();
    } catch {
      // A mount with no layout yet - the next resize does it.
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

/** Close one tab, stopping its run first if it is still going. */
export async function close(hostId: string, taskId: string): Promise<void> {
  const entry = hosts.get(hostId);
  const run = find(hostId, taskId);
  if (!entry || !run) return;

  if (run.state === "running") await stop(hostId, taskId);
  disposeRun(run);

  const index = entry.runs.indexOf(run);
  entry.runs.splice(index, 1);
  if (entry.active === taskId) {
    // The neighbour the eye lands on: the next tab, or the last one.
    entry.active = entry.runs[Math.min(index, entry.runs.length - 1)]?.taskId ?? null;
  }
  if (entry.runs.length === 0) hosts.delete(hostId);
  notify();
}

/** Everything this host had. Called on disconnect, reap, delete and app exit. */
export async function closeHost(hostId: string): Promise<void> {
  const entry = hosts.get(hostId);
  if (!entry) return;
  hosts.delete(hostId);

  for (const run of entry.runs) {
    if (run.state === "running" && run.streamId !== null) {
      await api.closeStream(hostId, run.streamId).catch(() => undefined);
    }
    disposeRun(run);
  }
  notify();
}

/** Follow the app's theme, like the terminals do. */
export function applyTheme(theme: "light" | "dark"): void {
  for (const entry of hosts.values()) {
    for (const run of entry.runs) run.terminal.options.theme = THEMES[theme];
  }
}
