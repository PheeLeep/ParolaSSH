import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { Alert, Button, Dropdown, Form, Spinner } from "react-bootstrap";
import { Eraser, Minus, Pencil, Plus, SquareTerminal, Type, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";
import { errorMessage } from "./api";
import * as store from "./terminalStore";
import {
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  TERMINAL_FONT_FAMILIES,
} from "../settings/preferences";
import { useTheme } from "../../theme/ThemeProvider";


export function TerminalTabs({
  hostId,
  focusShellId,
}: {
  hostId: string;
  /** A specific tab to open on, when Sessions linked to one shell. */
  focusShellId?: number;
}) {
  const { resolved } = useTheme();

  
  useSyncExternalStore(store.subscribe, store.getVersion);
  const terminals = store.forHost(hostId);

  const [activeId, setActiveId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountRef = useRef<HTMLDivElement>(null);

  const [renamingId, setRenamingId] = useState<number | null>(null);
  // A ref as well as state: the attach effect must see this without taking it
  // as a dependency, or renaming would tear the terminal down and rebuild it.
  const renamingRef = useRef(false);

  const startRename = useCallback((shellId: number) => {
    renamingRef.current = true;
    setActiveId(shellId);
    setRenamingId(shellId);
  }, []);

  // Renaming ends by handing the keyboard back to the terminal — the reason
  // you opened the tab in the first place.
  const endRename = useCallback((shellId: number) => {
    renamingRef.current = false;
    setRenamingId(null);
    store.focus(shellId);
  }, []);

  
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
      const linked = existing.find((entry) => entry.shellId === focusShellId);
      setActiveId((current) =>
        linked
          ? linked.shellId
          : current !== null && existing.some((entry) => entry.shellId === current)
            ? current
            : existing[0].shellId,
      );
      return;
    }
    setActiveId(null);
    void openRef.current();
  }, [hostId, focusShellId]);

  
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
    // Not while a rename is open: the input owns the keyboard until it closes.
    if (!renamingRef.current) store.focus(activeId);
    return detach;
  }, [activeId]);

  // A tab closing under an open rename would otherwise leave the field editing
  // a shell that no longer exists.
  useEffect(() => {
    if (renamingId !== null && !terminals.some((e) => e.shellId === renamingId)) {
      renamingRef.current = false;
      setRenamingId(null);
    }
  }, [terminals, renamingId]);

  useEffect(() => {
    store.applyTheme(resolved);
  }, [resolved]);

  const active = activeId === null ? undefined : store.get(activeId);

  return (
    <div className="terminal-pane">
      <div className="terminal-pane__bar">
        <div className="terminal-pane__tabs">
          {terminals.map((entry) => (
            <div
              key={entry.shellId}
              className={`shell-tab${entry.shellId === activeId ? " is-active" : ""}`}
            >
              {entry.shellId === renamingId ? (
                <RenameField
                  shellId={entry.shellId}
                  initial={entry.title}
                  onDone={() => endRename(entry.shellId)}
                />
              ) : (
                <button
                  type="button"
                  className="shell-tab__label"
                  onClick={() => setActiveId(entry.shellId)}
                  onDoubleClick={() => startRename(entry.shellId)}
                  onKeyDown={(event) => {
                    if (event.key === "F2") {
                      event.preventDefault();
                      startRename(entry.shellId);
                    }
                  }}
                  // F2 works too, but only while the tab itself holds focus —
                  // clicking one hands the keyboard straight to the terminal,
                  // so promising it in the tooltip would be a lie for most.
                  title={
                    entry.exited
                      ? "Session ended — double-click to rename"
                      : `${entry.title} — double-click to rename`
                  }
                >
                  <SquareTerminal className="icon-sm" aria-hidden="true" />
                  {entry.title}
                  {entry.exited && (
                    <span className="shell-tab__exit">
                      {entry.exitCode === null ? "ended" : entry.exitCode}
                    </span>
                  )}
                </button>
              )}
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
        </div>

        {/* Outside the scrolling strip: these stay put however many tabs are
            open, and a dropdown here is not clipped by its overflow. */}
        {active && (
          <div className="terminal-pane__actions">
            <FontMenu shellId={active.shellId} />
            <Button
              size="sm"
              variant="outline-secondary"
              onClick={() => startRename(active.shellId)}
              title="Rename this tab"
              aria-label="Rename this tab"
            >
              <Pencil aria-hidden="true" />
            </Button>
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

/**
 * The tab label, while it is being renamed.
 *
 * Enter and blur commit, Escape restores what was there before — the shape
 * every rename-in-place has, so it needs no explanation in the UI. An empty
 * name is not rejected: the store reads it as "put the original back", which
 * is the only other thing someone clearing the field could mean.
 */
function RenameField({
  shellId,
  initial,
  onDone,
}: {
  shellId: number;
  initial: string;
  onDone: () => void;
}) {
  const [value, setValue] = useState(initial);
  // Escape must not be undone by the blur that follows it.
  const cancelled = useRef(false);

  const commit = () => {
    if (cancelled.current) return;
    store.rename(shellId, value);
    onDone();
  };

  return (
    <input
      className="shell-tab__rename"
      value={value}
      maxLength={store.MAX_TERMINAL_TITLE}
      autoFocus
      onFocus={(event) => event.target.select()}
      onChange={(event) => setValue(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        // The terminal is not listening yet, but the tab strip is.
        event.stopPropagation();
        if (event.key === "Enter") {
          commit();
        } else if (event.key === "Escape") {
          cancelled.current = true;
          onDone();
        }
      }}
      aria-label="Tab name"
    />
  );
}

/**
 * Type controls for the active tab only. Settings holds the default every
 * new terminal starts from; this is for the one session where you need the
 * text bigger, without dragging every other terminal along.
 */
function FontMenu({ shellId }: { shellId: number }) {
  useSyncExternalStore(store.subscribe, store.getVersion);

  const font = store.fontFor(shellId);
  const overridden = store.hasFontOverride(shellId);

  const step = (delta: number) =>
    store.setFontOverride(shellId, { size: font.size + delta });

  return (
    // "outside" so stepping the size a few times doesn't shut the menu.
    <Dropdown align="end" autoClose="outside">
      <Dropdown.Toggle
        size="sm"
        variant="outline-secondary"
        className="no-caret"
        title={
          overridden
            ? `Font — ${font.size}px, this terminal only`
            : "Font for this terminal"
        }
        aria-label="Font for this terminal"
      >
        <Type aria-hidden="true" />
      </Dropdown.Toggle>

      <Dropdown.Menu className="terminal-font-menu">
        <div className="terminal-font-menu__row">
          <span className="terminal-font-menu__label">Size</span>
          <div className="d-flex align-items-center gap-1">
            <Button
              size="sm"
              variant="outline-secondary"
              disabled={font.size <= MIN_TERMINAL_FONT_SIZE}
              onClick={() => step(-1)}
              aria-label="Smaller"
            >
              <Minus aria-hidden="true" />
            </Button>
            <span className="terminal-font-menu__size">{font.size}px</span>
            <Button
              size="sm"
              variant="outline-secondary"
              disabled={font.size >= MAX_TERMINAL_FONT_SIZE}
              onClick={() => step(1)}
              aria-label="Larger"
            >
              <Plus aria-hidden="true" />
            </Button>
          </div>
        </div>

        <div className="terminal-font-menu__row">
          <span className="terminal-font-menu__label">Family</span>
          <Form.Select
            size="sm"
            value={font.family}
            onChange={(event) =>
              store.setFontOverride(shellId, { family: event.target.value })
            }
            aria-label="Font family for this terminal"
          >
            {TERMINAL_FONT_FAMILIES.every(
              (option) => option.value !== font.family,
            ) && <option value={font.family}>Custom</option>}
            {TERMINAL_FONT_FAMILIES.map((option) => (
              <option key={option.label} value={option.value}>
                {option.label}
              </option>
            ))}
          </Form.Select>
        </div>

        <Dropdown.Divider />
        <Dropdown.Item
          as="button"
          disabled={!overridden}
          onClick={() => store.setFontOverride(shellId, null)}
        >
          Use the default
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown>
  );
}
