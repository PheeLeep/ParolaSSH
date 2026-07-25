import { useEffect, useRef, useState } from "react";
import { Alert, Badge, Button, Spinner } from "react-bootstrap";
import { Eraser, SquareTerminal, X } from "lucide-react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import * as api from "./api";
import { errorMessage } from "./api";
import { useTheme } from "../../theme/ThemeProvider";

/** Matches the app's own palette so the terminal is not a bright hole in a
 *  dark window. Only the background and foreground follow the theme; the
 *  sixteen ANSI colours are fixed, because remote programs pick them by index
 *  and expect the usual meanings. */
const THEMES = {
  dark: { background: "#12151c", foreground: "#d7dce5", cursor: "#7aa2f7", selectionBackground: "#2a3350" },
  light: { background: "#ffffff", foreground: "#1f2430", cursor: "#3355cc", selectionBackground: "#cfd8f5" },
};

/**
 * A live shell for one host.
 *
 * xterm.js owns the DOM node; React only mounts it. Output arrives as Tauri
 * events rather than being polled, which is what makes full-screen programs
 * like `top` and `vim` usable instead of a slideshow.
 */
export function TerminalPane({
  hostId,
  onClose,
}: {
  hostId: string;
  onClose?: () => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  /** The shell this pane is currently showing, or null before it opens. */
  const shellIdRef = useRef<number | null>(null);

  const { resolved } = useTheme();
  const [status, setStatus] = useState<"opening" | "open" | "closed">("opening");
  const [error, setError] = useState<string | null>(null);
  const [exitCode, setExitCode] = useState<number | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let disposed = false;
    const unlisteners: Array<() => void> = [];

    // Which shell this pane owns. Events for any other shell on the same host
    // belong to a session we replaced and must not be rendered here. It lives
    // in a ref so `reopen` below can point the same listeners at a new shell.
    const isMine = (shellId: number) => shellIdRef.current === shellId;

    // Teardown has to wait for the open to resolve. Unmounting mid-open —
    // which StrictMode does on every mount in development — would otherwise
    // close nothing, and the shell that arrived a moment later would be
    // orphaned: still authenticated, still streaming into the next pane.
    let opening: Promise<number | null> = Promise.resolve(null);

    const terminal = new Terminal({
      fontFamily:
        'ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      // Enough history to scroll back through a build log without eating
      // memory for the lifetime of the app.
      scrollback: 5000,
      allowProposedApi: true,
      theme: THEMES[resolved],
    });

    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
    fit.fit();

    terminalRef.current = terminal;
    fitRef.current = fit;

    // Keystrokes go straight to the remote PTY; nothing is interpreted here,
    // so Ctrl-C and friends reach the process rather than the browser.
    terminal.onData((data) => {
      void api.writeShell(hostId, data).catch((caught) => {
        if (!disposed) setError(errorMessage(caught));
      });
    });

    const start = async () => {
      try {
        unlisteners.push(
          await api.onTerminalOutput(hostId, isMine, ({ chunk }) => {
            terminal.write(chunk);
          }),
        );
        unlisteners.push(
          await api.onTerminalClosed(hostId, isMine, ({ exitCode }) => {
            if (disposed) return;
            setStatus("closed");
            setExitCode(exitCode);
            terminal.write("\r\n\x1b[90m— session ended —\x1b[0m\r\n");
          }),
        );

        // Listeners are attached first: a shell that greets us immediately
        // would otherwise have its banner dropped. They stay inert until the
        // id below is claimed, so they cannot pick up a predecessor's output.
        opening = api.openShell(hostId, terminal.cols, terminal.rows);
        const shellId = await opening;

        // If the pane went away mid-open, leave the ref alone — a newer mount
        // may already own it — and let the cleanup close this shell instead.
        if (disposed) return;

        shellIdRef.current = shellId;
        setStatus("open");
        terminal.focus();
      } catch (caught) {
        if (!disposed) {
          setError(errorMessage(caught));
          setStatus("closed");
        }
      }
    };

    void start();

    // Keep the remote PTY the same size as the pane, or full-screen programs
    // wrap at whatever width they were started with.
    const observer = new ResizeObserver(() => {
      if (disposed) return;
      try {
        fit.fit();
        void api.resizeShell(hostId, terminal.cols, terminal.rows).catch(() => undefined);
      } catch {
        // A fit against a hidden pane throws; the next resize will correct it.
      }
    });
    observer.observe(container);

    return () => {
      disposed = true;
      observer.disconnect();
      for (const unlisten of unlisteners) unlisten();

      // Close only this pane's shell, and only once we know which one it is.
      // Quoting the id means a teardown that lands after a reopen closes
      // nothing rather than killing the session now on screen.
      void opening
        .then((shellId) =>
          shellId === null ? undefined : api.closeShell(hostId, shellId),
        )
        .catch(() => undefined);

      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
      shellIdRef.current = null;
    };
    // Re-creating the terminal on a theme change would wipe the scrollback, so
    // the theme is applied in the effect below instead.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId]);

  // Recolour in place when the app theme flips.
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    terminal.options.theme = THEMES[resolved];
  }, [resolved]);

  const reopen = async () => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    setError(null);
    setExitCode(null);
    setStatus("opening");
    try {
      shellIdRef.current = await api.openShell(hostId, terminal.cols, terminal.rows);
      setStatus("open");
      terminal.focus();
    } catch (caught) {
      setError(errorMessage(caught));
      setStatus("closed");
    }
  };

  return (
    <div className="terminal-pane">
      <div className="terminal-pane__bar">
        <SquareTerminal className="icon-sm" aria-hidden="true" />
        <span className="fw-semibold">Terminal</span>

        {status === "opening" && (
          <Spinner animation="border" size="sm" aria-label="Opening shell" />
        )}
        {status === "closed" && (
          <Badge bg="secondary">
            {exitCode === null ? "Closed" : `Exited ${exitCode}`}
          </Badge>
        )}

        <div className="ms-auto d-flex gap-1">
          {status === "closed" && (
            <Button size="sm" variant="outline-secondary" onClick={reopen}>
              Reopen
            </Button>
          )}
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={() => terminalRef.current?.clear()}
            aria-label="Clear the terminal"
          >
            <Eraser aria-hidden="true" />
          </Button>
          {onClose && (
            <Button
              size="sm"
              variant="outline-secondary"
              onClick={onClose}
              aria-label="Close the terminal"
            >
              <X aria-hidden="true" />
            </Button>
          )}
        </div>
      </div>

      {error && (
        <Alert variant="danger" className="m-2 mb-0 py-2 small text-prewrap">
          {error}
        </Alert>
      )}

      <div ref={containerRef} className="terminal-pane__screen" />
    </div>
  );
}
