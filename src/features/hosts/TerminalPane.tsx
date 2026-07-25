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

  const { resolved } = useTheme();
  const [status, setStatus] = useState<"opening" | "open" | "closed">("opening");
  const [error, setError] = useState<string | null>(null);
  const [exitCode, setExitCode] = useState<number | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let disposed = false;
    const unlisteners: Array<() => void> = [];

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
          await api.onTerminalOutput(hostId, ({ chunk }) => {
            terminal.write(chunk);
          }),
        );
        unlisteners.push(
          await api.onTerminalClosed(hostId, ({ exitCode }) => {
            if (disposed) return;
            setStatus("closed");
            setExitCode(exitCode);
            terminal.write("\r\n\x1b[90m— session ended —\x1b[0m\r\n");
          }),
        );

        // Listeners are attached first: a shell that greets us immediately
        // would otherwise have its banner dropped.
        await api.openShell(hostId, terminal.cols, terminal.rows);
        if (!disposed) {
          setStatus("open");
          terminal.focus();
        }
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
      void api.closeShell(hostId).catch(() => undefined);
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
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
      await api.openShell(hostId, terminal.cols, terminal.rows);
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
