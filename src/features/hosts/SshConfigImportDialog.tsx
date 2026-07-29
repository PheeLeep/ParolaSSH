import { useEffect, useMemo, useState } from "react";
import { Alert, Badge, Button, Form, Modal, Spinner } from "react-bootstrap";
import { Import, RefreshCw } from "lucide-react";
import * as api from "./api";
import { errorMessage } from "./api";
import { useHosts } from "./HostsProvider";
import {
  AUTH_METHOD_LABELS,
  DEFAULT_GROUP,
  draftFromHost,
  type AuthMethod,
  type ImportCandidate,
  type ImportListing,
  type SshHost,
} from "./types";

/**
 * Import the `Host` blocks of `~/.ssh/config` as saved connections.
 *
 * The config names an address and often a user and key, but never a password,
 * so a block that names no `User` takes the fallback typed here. Entries whose
 * address is already saved are shown and not selectable, so a second import
 * cannot duplicate them.
 */
export function SshConfigImportDialog({
  show,
  onClose,
}: {
  show: boolean;
  onClose: () => void;
}) {
  const { hosts, save } = useHosts();

  const [listing, setListing] = useState<ImportListing | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [username, setUsername] = useState("");
  const [authMethod, setAuthMethod] = useState<AuthMethod>("agent");
  const [group, setGroup] = useState("SSH config");
  const [importing, setImporting] = useState(false);
  const [imported, setImported] = useState<{ count: number; jumps: number } | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      setListing(await api.sshConfigHosts());
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  };

  // Re-read on open: the file is edited outside the app.
  useEffect(() => {
    if (!show) return;
    setSelected(new Set());
    setImported(null);
    void load();
  }, [show]);

  const candidates = listing?.candidates ?? [];

  /** Matched on address, port and user, since one machine legitimately appears
   *  under several aliases. */
  const savedKeys = useMemo(
    () => new Set(hosts.map((host) => addressKey(host.hostname, host.port, host.username))),
    [hosts],
  );

  const resolvedUser = (candidate: ImportCandidate) =>
    candidate.username || username.trim();

  const isSaved = (candidate: ImportCandidate) =>
    savedKeys.has(
      addressKey(candidate.hostname, candidate.port, resolvedUser(candidate)),
    );

  const importable = candidates.filter((candidate) => !isSaved(candidate));

  /** Entries with no `User` of their own cannot be saved without the fallback. */
  const needsUsername = importable.some(
    (candidate) => selected.has(candidate.alias) && !candidate.username,
  );

  const canImport = selected.size > 0 && !needsUsername && !importing;

  /** Selected entries whose jump host is not being imported and is not already
   *  saved - those land as direct connections until one is chosen by hand. */
  const unresolvedJumps = importable.filter(
    (candidate) =>
      selected.has(candidate.alias) &&
      candidate.proxyJump !== null &&
      !selected.has(candidate.proxyJump) &&
      !findSavedByAlias(hosts, candidates, candidate.proxyJump),
  );

  const toggle = (alias: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(alias)) next.delete(alias);
      else next.add(alias);
      return next;
    });
  };

  const toggleAll = () => {
    setSelected((current) =>
      current.size === importable.length
        ? new Set()
        : new Set(importable.map((candidate) => candidate.alias)),
    );
  };

  const onImport = async () => {
    setImporting(true);
    setError(null);
    try {
      const chosen = importable.filter((candidate) => selected.has(candidate.alias));

      // Sequential: each save rewrites the store, so concurrent writes would
      // race for the same file.
      const savedByAlias = new Map<string, SshHost>();
      for (const candidate of chosen) {
        const record = await save({
          label: candidate.alias,
          hostname: candidate.hostname,
          port: candidate.port,
          username: resolvedUser(candidate),
          authMethod: candidate.keyPath ? "publickey" : authMethod,
          keyPath: candidate.keyPath,
          group: group.trim() || DEFAULT_GROUP,
          tags: [],
          notes: `Imported from ${listing?.path ?? "ssh_config"} (line ${candidate.line})`,
          // Set on a second pass: a jump host has no id until it is saved.
          proxyJump: null,
        });
        savedByAlias.set(candidate.alias, record);
      }

      let jumps = 0;
      for (const candidate of chosen) {
        if (!candidate.proxyJump) continue;
        const record = savedByAlias.get(candidate.alias);
        const jumpHost =
          savedByAlias.get(candidate.proxyJump) ??
          findSavedByAlias(hosts, candidates, candidate.proxyJump);
        if (!record || !jumpHost || jumpHost.id === record.id) continue;

        await save({ ...draftFromHost(record), proxyJump: jumpHost.id });
        jumps += 1;
      }

      setImported({ count: chosen.length, jumps });
      setSelected(new Set());
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setImporting(false);
    }
  };

  return (
    <Modal show={show} onHide={onClose} size="lg" centered>
      <Modal.Header closeButton>
        <Modal.Title className="h6 mb-0">Import from ~/.ssh/config</Modal.Title>
      </Modal.Header>

      <Modal.Body>
        {error && (
          <Alert variant="danger" className="py-2 small text-prewrap">
            {error}
          </Alert>
        )}

        {imported !== null && (
          <Alert variant="success" className="py-2 small">
            Added {imported.count} {imported.count === 1 ? "host" : "hosts"}
            {imported.jumps > 0 &&
              `, ${imported.jumps} of them through a jump host`}
            . Passwords and passphrases are never read from the config, so set
            those on each host as you connect.
          </Alert>
        )}

        {loading && (
          <div className="d-flex align-items-center gap-2 text-body-secondary small">
            <Spinner animation="border" size="sm" aria-hidden="true" />
            Reading your SSH config…
          </div>
        )}

        {!loading && listing && !listing.exists && (
          <p className="text-body-secondary mb-0">
            There is no config file at <code>{listing.path}</code>.
          </p>
        )}

        {!loading && listing?.exists && candidates.length === 0 && (
          <p className="text-body-secondary mb-0">
            That config defines no single-machine <code>Host</code> blocks.
            Patterns like <code>Host *</code> describe a set of machines rather
            than an address, so there is nothing to import from them.
          </p>
        )}

        {!loading &&
          listing?.notes.map((note) => (
            <Alert key={note} variant="warning" className="py-2 small">
              {note}
            </Alert>
          ))}

        {!loading && candidates.length > 0 && (
          <>
            <div className="d-flex flex-wrap gap-3 mb-3">
              <Form.Group className="flex-grow-1">
                <Form.Label className="small mb-1">Fallback username</Form.Label>
                <Form.Control
                  size="sm"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  placeholder="used where the config names no User"
                  isInvalid={needsUsername}
                  autoFocus
                />
              </Form.Group>

              <Form.Group>
                <Form.Label className="small mb-1">Authentication</Form.Label>
                <Form.Select
                  size="sm"
                  value={authMethod}
                  onChange={(event) => setAuthMethod(event.target.value as AuthMethod)}
                >
                  {(Object.keys(AUTH_METHOD_LABELS) as AuthMethod[]).map((method) => (
                    <option key={method} value={method}>
                      {AUTH_METHOD_LABELS[method]}
                    </option>
                  ))}
                </Form.Select>
              </Form.Group>

              <Form.Group>
                <Form.Label className="small mb-1">Group</Form.Label>
                <Form.Control
                  size="sm"
                  value={group}
                  onChange={(event) => setGroup(event.target.value)}
                />
              </Form.Group>
            </div>

            <p className="text-body-secondary small">
              An entry with its own <code>IdentityFile</code> keeps that key; the
              rest use the method chosen here.
            </p>

            {unresolvedJumps.length > 0 && (
              <Alert variant="warning" className="py-2 small">
                {unresolvedJumps.map((candidate) => candidate.alias).join(", ")}{" "}
                {unresolvedJumps.length === 1 ? "jumps" : "jump"} through a host
                that is not selected and not already saved. Those will be added
                as direct connections - select the jump host too, or set one
                afterwards.
              </Alert>
            )}

            <div className="d-flex align-items-center gap-2 mb-2">
              <Button
                size="sm"
                variant="link"
                className="p-0"
                onClick={toggleAll}
                disabled={importable.length === 0}
              >
                {selected.size === importable.length && importable.length > 0
                  ? "Clear selection"
                  : "Select all"}
              </Button>
              <span className="text-body-secondary small ms-auto">
                {candidates.length} {candidates.length === 1 ? "entry" : "entries"}
              </span>
              <Button
                size="sm"
                variant="outline-secondary"
                onClick={() => void load()}
                aria-label="Re-read the config file"
              >
                <RefreshCw aria-hidden="true" />
              </Button>
            </div>

            <ul className="peer-list">
              {candidates.map((candidate) => {
                const saved = isSaved(candidate);
                const user = resolvedUser(candidate);
                return (
                  <li key={candidate.alias} className="peer-list__row">
                    <Form.Check
                      type="checkbox"
                      id={`config-${candidate.alias}`}
                      checked={selected.has(candidate.alias)}
                      disabled={saved}
                      onChange={() => toggle(candidate.alias)}
                      label={
                        <span className="d-flex align-items-center gap-2 flex-wrap">
                          <span className="fw-semibold">{candidate.alias}</span>
                          <code className="small">
                            {user ? `${user}@` : ""}
                            {candidate.hostname}:{candidate.port}
                          </code>
                          {!candidate.username && (
                            <span className="text-body-secondary small">
                              no User set
                            </span>
                          )}
                          {candidate.keyPath && (
                            <Badge bg="secondary" className="fw-normal">
                              {candidate.keyPath}
                            </Badge>
                          )}
                          {candidate.proxyJump && (
                            <Badge bg="info" className="fw-normal">
                              via {candidate.proxyJump}
                            </Badge>
                          )}
                          {saved && (
                            <span className="text-body-secondary small">
                              already saved
                            </span>
                          )}
                          {candidate.notes.map((note) => (
                            <span key={note} className="text-warning small">
                              {note}
                            </span>
                          ))}
                        </span>
                      }
                    />
                  </li>
                );
              })}
            </ul>
          </>
        )}
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onClose}>
          Close
        </Button>
        <Button variant="primary" onClick={() => void onImport()} disabled={!canImport}>
          {importing ? (
            <Spinner animation="border" size="sm" aria-hidden="true" />
          ) : (
            <Import aria-hidden="true" />
          )}
          Import {selected.size > 0 ? selected.size : ""}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}

/** Hostnames fold case on every platform we target; users do not. */
function addressKey(hostname: string, port: number, username: string) {
  return `${hostname.toLowerCase()}:${port}:${username.toLowerCase()}`;
}

/** A previously imported host for `alias`, matched by its label first and then
 *  by the address that alias resolves to. */
function findSavedByAlias(
  hosts: SshHost[],
  candidates: ImportCandidate[],
  alias: string,
): SshHost | undefined {
  const byLabel = hosts.find(
    (host) => host.label.toLowerCase() === alias.toLowerCase(),
  );
  if (byLabel) return byLabel;

  const candidate = candidates.find((entry) => entry.alias === alias);
  if (!candidate) return undefined;

  return hosts.find(
    (host) =>
      host.hostname.toLowerCase() === candidate.hostname.toLowerCase() &&
      host.port === candidate.port,
  );
}
