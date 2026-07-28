import { useEffect, useMemo, useState } from "react";
import { Alert, Badge, Button, Form, Modal, Spinner } from "react-bootstrap";
import { Import, RefreshCw } from "lucide-react";
import { errorMessage } from "../hosts/api";
import { useHosts } from "../hosts/HostsProvider";
import {
  AUTH_METHOD_LABELS,
  DEFAULT_GROUP,
  DEFAULT_PORT,
  type AuthMethod,
} from "../hosts/types";
import { tailscalePeers } from "./api";
import type { PeerListing, TailscalePeer } from "./types";

/**
 * Import tailnet machines as saved hosts.
 *
 * Tailscale knows the address, not the account, so one username and auth
 * method are applied to every selection - tailnets are usually administered
 * with one login, and anything unusual is a normal edit afterwards. Peers
 * already saved are shown but not selectable, so a second import cannot
 * silently duplicate them.
 */
export function TailscaleImportDialog({
  show,
  onClose,
}: {
  show: boolean;
  onClose: () => void;
}) {
  const { hosts, save } = useHosts();

  const [listing, setListing] = useState<PeerListing | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [username, setUsername] = useState("");
  // Not `none`: Tailscale SSH's server is Linux-only *and* opt-in, so it is
  // wrong more often than right as a batch default. It stays selectable, with
  // the peers it cannot serve named - see `noneCannotServe`.
  const [authMethod, setAuthMethod] = useState<AuthMethod>("agent");
  const [group, setGroup] = useState("Tailnet");
  const [importing, setImporting] = useState(false);
  const [imported, setImported] = useState<number | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      setListing(await tailscalePeers());
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  };

  // Re-read on open: peers come and go, and a stale list would offer machines
  // that have since left the tailnet.
  useEffect(() => {
    if (!show) return;
    setSelected(new Set());
    setImported(null);
    void load();
  }, [show]);

  const peers = listing?.kind === "peers" ? listing.peers : [];

  /** Any address this peer answers to, so a re-import cannot duplicate it. */
  const savedAddresses = useMemo(
    () => new Set(hosts.map((host) => host.hostname.toLowerCase())),
    [hosts],
  );

  const isSaved = (peer: TailscalePeer) =>
    [peer.address, peer.dnsName, peer.tailscaleIp, peer.hostName]
      .filter((value): value is string => Boolean(value))
      .some((value) => savedAddresses.has(value.toLowerCase()));

  const importable = peers.filter((peer) => !isSaved(peer));
  const canImport = selected.size > 0 && username.trim().length > 0 && !importing;

  /** Selected peers that cannot run a Tailscale SSH server at all - its server
   *  component is Linux-only, so `none` would never authenticate there. */
  const noneCannotServe =
    authMethod === "none"
      ? importable.filter(
          (peer) =>
            selected.has(peer.address) && peer.os.toLowerCase() !== "linux",
        )
      : [];

  const toggle = (address: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(address)) next.delete(address);
      else next.add(address);
      return next;
    });
  };

  const toggleAll = () => {
    setSelected((current) =>
      current.size === importable.length
        ? new Set()
        : new Set(importable.map((peer) => peer.address)),
    );
  };

  const onImport = async () => {
    setImporting(true);
    setError(null);
    try {
      const chosen = importable.filter((peer) => selected.has(peer.address));
      // Sequential: each save rewrites the store, so concurrent writes would
      // race for the same file.
      for (const peer of chosen) {
        await save({
          label: peer.hostName || peer.address,
          hostname: peer.address,
          port: DEFAULT_PORT,
          username: username.trim(),
          authMethod,
          keyPath: null,
          group: group.trim() || DEFAULT_GROUP,
          tags: peer.tags,
          notes: `Imported from Tailscale${peer.os ? ` · ${peer.os}` : ""}`,
        });
      }
      setImported(chosen.length);
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
        <Modal.Title className="h6 mb-0">Import from Tailscale</Modal.Title>
      </Modal.Header>

      <Modal.Body>
        {error && (
          <Alert variant="danger" className="py-2 small text-prewrap">
            {error}
          </Alert>
        )}

        {imported !== null && (
          <Alert variant="success" className="py-2 small">
            Added {imported} {imported === 1 ? "host" : "hosts"}. Set a key or
            password on each from its own page.
          </Alert>
        )}

        {loading && (
          <div className="d-flex align-items-center gap-2 text-body-secondary small">
            <Spinner animation="border" size="sm" aria-hidden="true" />
            Reading the tailnet…
          </div>
        )}

        {!loading && listing?.kind === "notInstalled" && (
          <p className="text-body-secondary mb-0">
            The Tailscale CLI is not on this machine.
          </p>
        )}

        {!loading && listing?.kind === "unavailable" && (
          <p className="text-body-secondary mb-0">
            No peer list - {listing.detail}. ParolaSSH does not log in or start
            the client for you.
          </p>
        )}

        {!loading && listing?.kind === "peers" && peers.length === 0 && (
          <p className="text-body-secondary mb-0">
            This tailnet has no other machines.
          </p>
        )}

        {!loading && peers.length > 0 && (
          <>
            <div className="d-flex flex-wrap gap-3 mb-3">
              <Form.Group className="flex-grow-1">
                <Form.Label className="small mb-1">Username</Form.Label>
                <Form.Control
                  size="sm"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  placeholder="the account on these machines"
                  autoFocus
                />
              </Form.Group>

              <Form.Group>
                <Form.Label className="small mb-1">Authentication</Form.Label>
                <Form.Select
                  size="sm"
                  value={authMethod}
                  onChange={(event) =>
                    setAuthMethod(event.target.value as AuthMethod)
                  }
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

            {authMethod === "none" && (
              <p className="text-body-secondary small mb-1">
                For Tailscale SSH, which identifies the node over WireGuard and
                asks for no credential. It is opt-in per node, and a node
                running an ordinary sshd needs a password or key instead.
              </p>
            )}

            {noneCannotServe.length > 0 && (
              <Alert variant="warning" className="py-2 small">
                Tailscale SSH's server runs only on Linux, so{" "}
                {noneCannotServe.map((peer) => peer.hostName).join(", ")} cannot
                accept it - those will need a password or key.
              </Alert>
            )}

            {authMethod === "publickey" && (
              <p className="text-body-secondary small">
                Imported hosts get no key file - choose one per host afterwards.
              </p>
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
                {peers.length} {peers.length === 1 ? "machine" : "machines"}
              </span>
              <Button
                size="sm"
                variant="outline-secondary"
                onClick={() => void load()}
                aria-label="Re-read the tailnet"
              >
                <RefreshCw aria-hidden="true" />
              </Button>
            </div>

            <ul className="peer-list">
              {peers.map((peer) => {
                const saved = isSaved(peer);
                return (
                  <li key={peer.address} className="peer-list__row">
                    <Form.Check
                      type="checkbox"
                      id={`peer-${peer.address}`}
                      checked={selected.has(peer.address)}
                      disabled={saved}
                      onChange={() => toggle(peer.address)}
                      label={
                        <span className="d-flex align-items-center gap-2 flex-wrap">
                          <span
                            className={`status-dot status-dot--${peer.online ? "connected" : "offline"}`}
                            aria-hidden="true"
                          />
                          <span className="fw-semibold">{peer.hostName}</span>
                          <code className="small">{peer.address}</code>
                          {peer.os && (
                            <span className="text-body-secondary small">{peer.os}</span>
                          )}
                          {peer.tags.map((tag) => (
                            <Badge key={tag} bg="secondary" className="fw-normal">
                              {tag}
                            </Badge>
                          ))}
                          {saved && (
                            <span className="text-body-secondary small">
                              already saved
                            </span>
                          )}
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
