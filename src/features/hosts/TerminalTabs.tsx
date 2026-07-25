import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { Alert, Button, Spinner } from "react-bootstrap";
import { Eraser, Plus, SquareTerminal, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";
import { errorMessage } from "./api";
import * as store from "./terminalStore";
import { useTheme } from "../../theme/ThemeProvider";


export function TerminalTabs({ hostId }: { hostId: string }) {
  const { resolved } = useTheme();

  
  useSyncExternalStore(store.subscribe, store.getVersion);
  const terminals = store.forHost(hostId);

  const [activeId, setActiveId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountRef = useRef<HTMLDivElement>(null);

  
  const openingRef = useRef(false);

  const openTerminal = useCallback(async () => {
    if (openingRef.current) return;
    openingRef.current = true;
    setBusy(true);
    setError(null);
    try {
      setActiveId(await store.open(hostId, resolved));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      openingRef.current = false;
      setBusy(false);
    }
  }, [hostId, resolved]);

  
  const openRef = useRef(openTerminal);
  useEffect(() => {
    openRef.current = openTerminal;
  }, [openTerminal]);

  
  useEffect(() => {
    const existing = store.forHost(hostId);
    if (existing.length > 0) {
      setActiveId((current) =>
        current !== null && existing.some((entry) => entry.shellId === current)
          ? current
          : existing[0].shellId,
      );
      return;
    }
    setActiveId(null);
    void openRef.current();
  }, [hostId]);

  
  useEffect(() => {
    if (terminals.length === 0) {
      if (activeId !== null) setActiveId(null);
      return;
    }
    if (activeId === null || !terminals.some((entry) => entry.shellId === activeId)) {
      setActiveId(terminals[terminals.length - 1].shellId);
    }
  }, [terminals, activeId]);

  
  useEffect(() => {
    const mount = mountRef.current;
    if (!mount || activeId === null) return;

    const detach = store.attach(activeId, mount);
    store.focus(activeId);
    return detach;
  }, [activeId]);

  useEffect(() => {
    store.applyTheme(resolved);
  }, [resolved]);

  const active = activeId === null ? undefined : store.get(activeId);

  return (
    <div className="terminal-pane">
      <div className="terminal-pane__tabs">
        {terminals.map((entry) => (
          <div
            key={entry.shellId}
            className={`shell-tab${entry.shellId === activeId ? " is-active" : ""}`}
          >
            <button
              type="button"
              className="shell-tab__label"
              onClick={() => setActiveId(entry.shellId)}
              title={entry.exited ? "Session ended" : entry.title}
            >
              <SquareTerminal className="icon-sm" aria-hidden="true" />
              {entry.title}
              {entry.exited && (
                <span className="shell-tab__exit">
                  {entry.exitCode === null ? "ended" : entry.exitCode}
                </span>
              )}
            </button>
            <button
              type="button"
              className="shell-tab__close"
              onClick={() => void store.close(entry.shellId)}
              aria-label={`Close ${entry.title}`}
            >
              <X aria-hidden="true" />
            </button>
          </div>
        ))}

        <button
          type="button"
          className="shell-tab__add"
          onClick={() => void openTerminal()}
          disabled={busy}
          aria-label="New terminal"
          title="New terminal on this host"
        >
          {busy ? (
            <Spinner animation="border" size="sm" aria-hidden="true" />
          ) : (
            <Plus aria-hidden="true" />
          )}
        </button>

        {active && (
          <div className="ms-auto d-flex gap-1 pe-1">
            <Button
              size="sm"
              variant="outline-secondary"
              onClick={() => store.clear(active.shellId)}
              aria-label="Clear this terminal"
            >
              <Eraser aria-hidden="true" />
            </Button>
          </div>
        )}
      </div>

      {error && (
        <Alert variant="danger" className="m-2 mb-0 py-2 small text-prewrap">
          {error}
        </Alert>
      )}

      {terminals.length === 0 && !busy ? (
        <div className="terminal-pane__screen terminal-pane__empty">
          <p className="mb-2">No terminal open on this host.</p>
          <Button size="sm" variant="primary" onClick={() => void openTerminal()}>
            <Plus aria-hidden="true" />
            Open a terminal
          </Button>
        </div>
      ) : (
        // Owned by the store, which appends the terminal's node here. React
        // must never render children into it.
        <div ref={mountRef} className="terminal-pane__screen" />
      )}
    </div>
  );
}
