import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Alert, Button, Card, Form, InputGroup, Spinner } from "react-bootstrap";
import { Copy, FolderOpen, RefreshCw, Search, Trash2 } from "lucide-react";
import { revealInFileManager } from "../../lib/openExternal";
import {
  appLogLocation,
  clearAppLog,
  readAppLog,
  type LogEntry,
  type LogLevel,
  type LogLocation,
} from "./logsApi";

/** Everything at or above the chosen level shows, so one control covers the
 *  usual "just the problems" and "everything" cases. */
const LEVEL_ORDER: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

const LEVEL_LABELS: { value: LogLevel; label: string }[] = [
  { value: "debug", label: "Everything" },
  { value: "info", label: "Info and above" },
  { value: "warn", label: "Warnings and errors" },
  { value: "error", label: "Errors only" },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function LogsPanel() {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [location, setLocation] = useState<LogLocation | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [minLevel, setMinLevel] = useState<LogLevel>("debug");
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextEntries, nextLocation] = await Promise.all([
        readAppLog(),
        appLogLocation(),
      ]);
      setEntries(nextEntries);
      setLocation(nextLocation);
      setError(null);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return entries.filter((entry) => {
      if (LEVEL_ORDER[entry.level] < LEVEL_ORDER[minLevel]) return false;
      if (!needle) return true;
      return (
        entry.message.toLowerCase().includes(needle) ||
        entry.target.toLowerCase().includes(needle)
      );
    });
  }, [entries, minLevel, query]);

  // Newest is last, so the view opens at the bottom - the same rule the
  // service journal follows.
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const node = listRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [visible]);

  const copy = async () => {
    const text = visible
      .map((entry) => `${entry.time}  ${entry.level.toUpperCase()}  ${entry.target}  ${entry.message}`)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be refused; the text is still on screen.
    }
  };

  const clear = async () => {
    await clearAppLog().catch(() => undefined);
    await refresh();
  };

  return (
    <Card className="mb-3">
      <Card.Body>
        <h2 className="section-title mb-1">Logs</h2>
        <p className="text-body-secondary small mb-3">
          What ParolaSSH did, kept on this machine so a failure can be read back
          later. Connections, power and service actions, and transfers - never
          passwords, key material, or output from a remote machine.
        </p>

        {error && <Alert variant="danger">{error}</Alert>}

        <div className="d-flex flex-wrap align-items-center gap-2 mb-3">
          <Form.Select
            size="sm"
            className="w-auto"
            value={minLevel}
            onChange={(event) => setMinLevel(event.target.value as LogLevel)}
            aria-label="Minimum level"
          >
            {LEVEL_LABELS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </Form.Select>

          <InputGroup size="sm" style={{ maxWidth: "16rem" }}>
            <InputGroup.Text>
              <Search className="icon-sm" aria-hidden="true" />
            </InputGroup.Text>
            <Form.Control
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter log"
              aria-label="Filter log"
            />
          </InputGroup>

          <Button
            size="sm"
            variant="outline-secondary"
            className="ms-auto"
            onClick={() => void refresh()}
            disabled={loading}
          >
            <RefreshCw aria-hidden="true" />
            Refresh
          </Button>
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={() => void copy()}
            disabled={visible.length === 0}
          >
            <Copy aria-hidden="true" />
            {copied ? "Copied" : "Copy"}
          </Button>
          <Button
            size="sm"
            variant="outline-secondary"
            disabled={!location?.exists}
            onClick={() => location && revealInFileManager(location.path)}
          >
            <FolderOpen aria-hidden="true" />
            Reveal
          </Button>
          <Button
            size="sm"
            variant="outline-danger"
            onClick={() => void clear()}
            disabled={entries.length === 0}
          >
            <Trash2 aria-hidden="true" />
            Clear
          </Button>
        </div>

        {loading && entries.length === 0 ? (
          <div className="d-flex align-items-center gap-2 text-body-secondary py-4">
            <Spinner animation="border" size="sm" aria-hidden="true" />
            Reading the log…
          </div>
        ) : visible.length === 0 ? (
          <p className="text-body-secondary small mb-0 py-3">
            {entries.length === 0
              ? "Nothing logged yet."
              : "No entries match this filter."}
          </p>
        ) : (
          <div className="log-view" ref={listRef}>
            {visible.map((entry, index) => (
              <div key={`${entry.time}-${index}`} className="log-line">
                <span className="log-line__time">{entry.time}</span>
                <span className={`log-line__level is-${entry.level}`}>
                  {entry.level}
                </span>
                <span className="log-line__target">{entry.target}</span>
                <span className="log-line__message">{entry.message}</span>
              </div>
            ))}
          </div>
        )}

        {location && (
          <p className="text-body-secondary small mt-3 mb-0">
            <code className="user-select-auto">{location.path}</code>
            {location.exists && <> · {formatBytes(location.bytes)}</>}
            {" · showing "}
            {visible.length} of {entries.length} recent entries
          </p>
        )}
      </Card.Body>
    </Card>
  );
}
