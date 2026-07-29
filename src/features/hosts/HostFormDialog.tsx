import { useEffect, useMemo, useState } from "react";
import { Alert, Button, Col, Form, Modal, Row, Spinner } from "react-bootstrap";
import { CircleCheck, CircleX, Plug, Server, TriangleAlert } from "lucide-react";
import * as api from "./api";
import { errorMessage } from "./api";
import { useHosts } from "./HostsProvider";
import { TagInput } from "./TagInput";
import { useKeys } from "../keys/KeysProvider";
import {
  AUTH_METHOD_LABELS,
  DEFAULT_GROUP,
  emptyDraft,
  type AuthMethod,
  type HostDraft,
  type ProbeResult,
} from "./types";

/**
 * Add or edit a connection.
 *
 * The same dialog does both: the only difference is whether the draft carries
 * an id, which is also the only thing the Rust side looks at to decide between
 * insert and update.
 */
export function HostFormDialog({
  draft: initialDraft,
  onClose,
  onSaved,
}: {
  /** `null` closes the dialog. A draft without an id is an add. */
  draft: HostDraft | null;
  onClose: () => void;
  onSaved?: (id: string) => void;
}) {
  const { hosts, save } = useHosts();
  const { keys } = useKeys();

  const [draft, setDraft] = useState<HostDraft>(emptyDraft());
  const [groups, setGroups] = useState<string[]>([]);
  const [tagSuggestions, setTagSuggestions] = useState<string[]>([]);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [probe, setProbe] = useState<ProbeResult | null>(null);
  const [probing, setProbing] = useState(false);

  const isEdit = Boolean(initialDraft?.id);

  /** Every saved host except this one and anything already jumping through it.
   *  A loop is refused at connect time; not offering one is kinder. */
  const jumpChoices = useMemo(() => {
    const self = initialDraft?.id;
    if (!self) return hosts;

    const jumpsThroughSelf = (start: string) => {
      const seen = new Set<string>();
      let id: string | null | undefined = start;
      while (id && !seen.has(id)) {
        if (id === self) return true;
        seen.add(id);
        id = hosts.find((host) => host.id === id)?.proxyJump;
      }
      return false;
    };

    return hosts.filter((host) => host.id !== self && !jumpsThroughSelf(host.id));
  }, [hosts, initialDraft?.id]);

  // Reset whenever the dialog opens, so a previous edit never bleeds through.
  useEffect(() => {
    if (!initialDraft) return;
    setDraft(initialDraft);
    setError(null);
    setProbe(null);
    setProbing(false);
    setBusy(false);

    void api.listHostGroups().then(setGroups).catch(() => setGroups([]));
    void api.listHostTags().then(setTagSuggestions).catch(() => setTagSuggestions([]));
  }, [initialDraft]);

  if (!initialDraft) return null;

  const update = <Field extends keyof HostDraft>(field: Field, value: HostDraft[Field]) =>
    setDraft((previous) => ({ ...previous, [field]: value }));

  const canSubmit =
    draft.hostname.trim().length > 0 && draft.username.trim().length > 0 && !busy;

  /** Check the port before saving - the commonest mistake is a wrong one. */
  const runProbe = async () => {
    setProbing(true);
    setProbe(null);
    try {
      setProbe(await api.probeHost(draft.hostname.trim(), draft.port));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setProbing(false);
    }
  };

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const saved = await save(draft);
      onSaved?.(saved.id);
      onClose();
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(false);
    }
  };

  return (
    <Modal show onHide={onClose} centered size="lg" backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <Server aria-hidden="true" />
          {isEdit ? `Edit ${initialDraft.label || "connection"}` : "New connection"}
        </Modal.Title>
      </Modal.Header>

      <Modal.Body>
        {error && <Alert variant="danger">{error}</Alert>}

        <Form.Group className="mb-3">
          <Form.Label>Name</Form.Label>
          <Form.Control
            value={draft.label}
            onChange={(event) => update("label", event.target.value)}
            placeholder="Test VM"
            autoFocus
            spellCheck={false}
          />
          <Form.Text className="text-body-secondary">
            Shown in the list. Left blank, it becomes <code>user@hostname</code>.
          </Form.Text>
        </Form.Group>

        <Row className="g-3 mb-3">
          <Col sm={8}>
            <Form.Group>
              <Form.Label>Hostname or IP</Form.Label>
              <Form.Control
                value={draft.hostname}
                onChange={(event) => update("hostname", event.target.value)}
                placeholder="192.168.56.10"
                spellCheck={false}
                autoComplete="off"
                required
              />
            </Form.Group>
          </Col>
          <Col sm={4}>
            <Form.Group>
              <Form.Label>Port</Form.Label>
              <Form.Control
                type="number"
                min={1}
                max={65535}
                value={draft.port}
                onChange={(event) =>
                  update("port", Number(event.target.value) || 0)
                }
              />
              <Form.Text className="text-body-secondary">
                22 unless moved.
              </Form.Text>
            </Form.Group>
          </Col>
        </Row>

        <div className="mb-3">
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={runProbe}
            disabled={probing || draft.hostname.trim().length === 0}
          >
            {probing ? (
              <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
            ) : (
              <Plug aria-hidden="true" />
            )}
            {probing ? "Checking…" : "Check port"}
          </Button>

          {probe && (
            <Alert
              variant={probe.isSsh ? "success" : probe.reachable ? "warning" : "danger"}
              className="mt-2 mb-0 d-flex gap-2 py-2 small"
            >
              {probe.isSsh ? (
                <CircleCheck className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
              ) : (
                <CircleX className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
              )}
              <div>
                {probe.message}
                {probe.latencyMs !== null && (
                  <span className="text-body-secondary"> ({probe.latencyMs} ms)</span>
                )}
              </div>
            </Alert>
          )}
        </div>

        <Row className="g-3 mb-3">
          <Col sm={6}>
            <Form.Group>
              <Form.Label>Username</Form.Label>
              <Form.Control
                value={draft.username}
                onChange={(event) => update("username", event.target.value)}
                placeholder="pheeleep"
                spellCheck={false}
                autoComplete="off"
                required
              />
            </Form.Group>
          </Col>
          <Col sm={6}>
            <Form.Group>
              <Form.Label>Authentication</Form.Label>
              <Form.Select
                value={draft.authMethod}
                onChange={(event) =>
                  update("authMethod", event.target.value as AuthMethod)
                }
              >
                {(Object.keys(AUTH_METHOD_LABELS) as AuthMethod[]).map((method) => (
                  <option key={method} value={method}>
                    {AUTH_METHOD_LABELS[method]}
                  </option>
                ))}
              </Form.Select>
            </Form.Group>
          </Col>
        </Row>

        {draft.authMethod === "publickey" && (
          <Form.Group className="mb-3">
            <Form.Label>Private key</Form.Label>
            <Form.Select
              value={draft.keyPath ?? ""}
              onChange={(event) => update("keyPath", event.target.value || null)}
            >
              <option value="">Choose a key…</option>
              {keys.map((key) => (
                <option key={key.path} value={key.path}>
                  {key.fileName} - {key.algorithm}
                </option>
              ))}
            </Form.Select>
            {keys.length === 0 && (
              <Form.Text className="text-warning">
                No keys found in your SSH directory. Generate one on the Keys
                page first.
              </Form.Text>
            )}
          </Form.Group>
        )}

        {draft.authMethod === "none" && (
          <Alert variant="secondary" className="d-flex gap-2 py-2 small">
            <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>
              No password or key is sent. Only works where the server already
              knows who you are - Tailscale SSH authenticates the node over
              WireGuard before SSH begins, and then asks for nothing. Its server
              is Linux-only and opt-in; an ordinary sshd, and any Windows host,
              will refuse this.
            </div>
          </Alert>
        )}

        {draft.authMethod === "password" && (
          <Alert variant="secondary" className="d-flex gap-2 py-2 small">
            <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>
              The password is asked for when you connect and is never written to
              disk. You can have it remembered until you quit the app.
            </div>
          </Alert>
        )}

        <Row className="g-3 mb-3">
          <Col sm={6}>
            <Form.Group controlId="host-group">
              <Form.Label>Group</Form.Label>
              <Form.Control
                value={draft.group}
                onChange={(event) => update("group", event.target.value)}
                placeholder={DEFAULT_GROUP}
                list="host-group-suggestions"
                spellCheck={false}
                autoComplete="off"
              />
              <datalist id="host-group-suggestions">
                {groups.map((group) => (
                  <option key={group} value={group} />
                ))}
              </datalist>
              <Form.Text className="text-body-secondary">
                Type a new name or pick an existing one. Groups are the folders
                in the sidebar.
              </Form.Text>
            </Form.Group>
          </Col>
          <Col sm={6}>
            <Form.Group controlId="host-tags">
              <Form.Label>Tags</Form.Label>
              <TagInput
                id="host-tags"
                value={draft.tags}
                onChange={(tags) => update("tags", tags)}
                suggestions={tagSuggestions}
              />
              <Form.Text className="text-body-secondary">
                Enter or comma to add. Tags are searchable from the host list.
              </Form.Text>
            </Form.Group>
          </Col>
        </Row>

        <Form.Group className="mb-3">
          <Form.Label>Jump host</Form.Label>
          <Form.Select
            value={draft.proxyJump ?? ""}
            onChange={(event) => update("proxyJump", event.target.value || null)}
          >
            <option value="">Connect directly</option>
            {jumpChoices.map((host) => (
              <option key={host.id} value={host.id}>
                {host.label} ({host.username}@{host.hostname})
              </option>
            ))}
          </Form.Select>
          <Form.Text className="text-body-secondary">
            Reaches this machine through another saved connection, the way ssh's{" "}
            <code>ProxyJump</code> does. The jump host uses its own credentials,
            so connect to it directly once first to record its host key.{" "}
            <strong>Check port</strong> above still probes from this machine and
            will say offline for a host only reachable through the jump.
          </Form.Text>
        </Form.Group>

        <Form.Group>
          <Form.Label>Notes</Form.Label>
          <Form.Control
            as="textarea"
            rows={2}
            value={draft.notes ?? ""}
            onChange={(event) => update("notes", event.target.value || null)}
            placeholder="Anything worth remembering about this host."
          />
        </Form.Group>
      </Modal.Body>

      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" onClick={submit} disabled={!canSubmit}>
          {busy && (
            <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
          )}
          {busy ? "Saving…" : isEdit ? "Save changes" : "Add connection"}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
