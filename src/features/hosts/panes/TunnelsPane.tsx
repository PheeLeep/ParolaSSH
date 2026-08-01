import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button, ButtonGroup, Card, Form, Stack } from "react-bootstrap";
import { ArrowLeft, ArrowRight, Cable, Plus, X } from "lucide-react";
import * as api from "../api";
import { errorMessage } from "../api";
import type { TunnelDirection, TunnelInfo } from "../types";

export function TunnelsPane({ hostId }: { hostId: string }) {
  const [tunnels, setTunnels] = useState<TunnelInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setTunnels(await api.listTunnels(hostId));
    } catch {
      // Not connected - list will be empty.
    }
  }, [hostId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    const promise = api.onTunnelEvent((event) => {
      if (!cancelled && event.hostId === hostId) void refresh();
    });
    return () => {
      cancelled = true;
      void promise.then((unlisten) => unlisten());
    };
  }, [hostId, refresh]);

  const close = async (tunnelId: number) => {
    try {
      await api.closeTunnel(hostId, tunnelId);
      await refresh();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  return (
    <div className="d-flex flex-column gap-3">
      {error && (
        <Alert variant="danger" dismissible onClose={() => setError(null)} className="py-2 small">
          {error}
        </Alert>
      )}

      <div className="d-flex align-items-center gap-2">
        <h2 className="h6 mb-0">Port forwarding</h2>
        <Button
          size="sm"
          variant="outline-primary"
          className="ms-auto"
          onClick={() => setShowForm(true)}
          disabled={showForm}
        >
          <Plus aria-hidden="true" />
          New tunnel
        </Button>
      </div>

      {showForm && (
        <NewTunnelForm
          hostId={hostId}
          onCreated={() => {
            setShowForm(false);
            void refresh();
          }}
          onCancel={() => setShowForm(false)}
          onError={setError}
        />
      )}

      {tunnels.length === 0 && !showForm && (
        <p className="text-body-secondary small mb-0">
          No active tunnels. Create one to forward a local port through this
          host to a remote address - like <code>ssh -L</code> or{" "}
          <code>ssh -R</code>.
        </p>
      )}

      {tunnels.length > 0 && (
        <Card>
          <Card.Body className="p-0">
            <div className="tunnel-list">
              {tunnels.map((tunnel) => (
                <TunnelRow
                  key={tunnel.id}
                  tunnel={tunnel}
                  onClose={() => void close(tunnel.id)}
                />
              ))}
            </div>
          </Card.Body>
        </Card>
      )}

      <Card>
        <Card.Body className="small text-body-secondary">
          <Cable className="icon-sm me-1" aria-hidden="true" />
          <strong>Local</strong> tunnels forward connections from a port on this
          machine through the SSH session to a remote address.{" "}
          <strong>Remote</strong> tunnels ask the server to listen on a port and
          forward connections back to an address on this machine. Closing the
          tunnel or disconnecting stops forwarding immediately.
        </Card.Body>
      </Card>
    </div>
  );
}

function TunnelRow({
  tunnel,
  onClose,
}: {
  tunnel: TunnelInfo;
  onClose: () => void;
}) {
  const isLocal = tunnel.direction === "local";
  const Arrow = isLocal ? ArrowRight : ArrowLeft;

  return (
    <div className="tunnel-list__row d-flex align-items-center gap-2 px-3 py-2">
      <Badge
        bg={isLocal ? "primary" : "info"}
        className="fw-normal text-uppercase"
        style={{ fontSize: "0.65rem" }}
      >
        {tunnel.direction}
      </Badge>
      {isLocal ? (
        <>
          <code className="small">127.0.0.1:{tunnel.localPort}</code>
          <Arrow className="icon-sm text-body-secondary flex-shrink-0" aria-hidden="true" />
          <code className="small">
            {tunnel.remoteHost}:{tunnel.remotePort}
          </code>
        </>
      ) : (
        <>
          <code className="small">
            {tunnel.remoteHost}:{tunnel.remotePort}
          </code>
          <Arrow className="icon-sm text-body-secondary flex-shrink-0" aria-hidden="true" />
          <code className="small">
            {tunnel.localHost}:{tunnel.localPort}
          </code>
        </>
      )}
      {tunnel.activeConnections > 0 && (
        <Badge bg="info" className="fw-normal">
          {tunnel.activeConnections} active
        </Badge>
      )}
      <Button
        size="sm"
        variant="outline-danger"
        className="ms-auto"
        onClick={onClose}
        title="Close this tunnel"
      >
        <X aria-hidden="true" />
      </Button>
    </div>
  );
}

function NewTunnelForm({
  hostId,
  onCreated,
  onCancel,
  onError,
}: {
  hostId: string;
  onCreated: () => void;
  onCancel: () => void;
  onError: (message: string) => void;
}) {
  const [direction, setDirection] = useState<TunnelDirection>("local");
  const [localPort, setLocalPort] = useState("");
  const [localHost, setLocalHost] = useState("127.0.0.1");
  const [remoteHost, setRemoteHost] = useState("127.0.0.1");
  const [remotePort, setRemotePort] = useState("");
  const [busy, setBusy] = useState(false);

  const isLocal = direction === "local";

  const canSubmit =
    !busy &&
    remotePort.trim() !== "" &&
    !isNaN(Number(remotePort)) &&
    (isLocal || (localPort.trim() !== "" && !isNaN(Number(localPort))));

  const submit = async () => {
    setBusy(true);
    try {
      if (isLocal) {
        await api.openTunnel(
          hostId,
          localPort.trim() === "" ? 0 : Number(localPort),
          remoteHost.trim() || "127.0.0.1",
          Number(remotePort),
        );
      } else {
        await api.openRemoteTunnel(
          hostId,
          Number(remotePort),
          remoteHost.trim() || "0.0.0.0",
          localHost.trim() || "127.0.0.1",
          Number(localPort),
        );
      }
      onCreated();
    } catch (caught) {
      onError(errorMessage(caught));
      setBusy(false);
    }
  };

  return (
    <Card>
      <Card.Body>
        <ButtonGroup size="sm" className="mb-3">
          <Button
            variant={isLocal ? "primary" : "outline-secondary"}
            onClick={() => setDirection("local")}
          >
            Local (-L)
          </Button>
          <Button
            variant={!isLocal ? "primary" : "outline-secondary"}
            onClick={() => setDirection("remote")}
          >
            Remote (-R)
          </Button>
        </ButtonGroup>

        <Stack direction="horizontal" gap={3} className="flex-wrap">
          {isLocal ? (
            <>
              <Form.Group>
                <Form.Label className="small mb-1">Local port</Form.Label>
                <Form.Control
                  size="sm"
                  type="number"
                  min={0}
                  max={65535}
                  placeholder="auto"
                  value={localPort}
                  onChange={(e) => setLocalPort(e.target.value)}
                  style={{ width: 100 }}
                />
              </Form.Group>

              <ArrowRight
                className="icon-sm text-body-secondary mt-4 flex-shrink-0"
                aria-hidden="true"
              />

              <Form.Group className="flex-grow-1">
                <Form.Label className="small mb-1">Remote host</Form.Label>
                <Form.Control
                  size="sm"
                  value={remoteHost}
                  onChange={(e) => setRemoteHost(e.target.value)}
                  placeholder="127.0.0.1"
                />
              </Form.Group>

              <Form.Group>
                <Form.Label className="small mb-1">Remote port</Form.Label>
                <Form.Control
                  size="sm"
                  type="number"
                  min={1}
                  max={65535}
                  value={remotePort}
                  onChange={(e) => setRemotePort(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && canSubmit) void submit();
                  }}
                  placeholder="e.g. 5432"
                  autoFocus
                  style={{ width: 100 }}
                />
              </Form.Group>
            </>
          ) : (
            <>
              <Form.Group>
                <Form.Label className="small mb-1">Server port</Form.Label>
                <Form.Control
                  size="sm"
                  type="number"
                  min={1}
                  max={65535}
                  value={remotePort}
                  onChange={(e) => setRemotePort(e.target.value)}
                  placeholder="e.g. 8080"
                  autoFocus
                  style={{ width: 100 }}
                />
              </Form.Group>

              <ArrowLeft
                className="icon-sm text-body-secondary mt-4 flex-shrink-0"
                aria-hidden="true"
              />

              <Form.Group className="flex-grow-1">
                <Form.Label className="small mb-1">Local host</Form.Label>
                <Form.Control
                  size="sm"
                  value={localHost}
                  onChange={(e) => setLocalHost(e.target.value)}
                  placeholder="127.0.0.1"
                />
              </Form.Group>

              <Form.Group>
                <Form.Label className="small mb-1">Local port</Form.Label>
                <Form.Control
                  size="sm"
                  type="number"
                  min={1}
                  max={65535}
                  value={localPort}
                  onChange={(e) => setLocalPort(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && canSubmit) void submit();
                  }}
                  placeholder="e.g. 3000"
                  style={{ width: 100 }}
                />
              </Form.Group>
            </>
          )}

          <div className="d-flex gap-2 mt-4">
            <Button size="sm" variant="primary" disabled={!canSubmit} onClick={() => void submit()}>
              Open
            </Button>
            <Button size="sm" variant="outline-secondary" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
          </div>
        </Stack>
        <Form.Text className="text-body-secondary">
          {isLocal
            ? "Leave local port blank to pick one automatically. Remote host is relative to the server - 127.0.0.1 means the server itself."
            : "The server listens on the server port and forwards connections to the local address on this machine."}
        </Form.Text>
      </Card.Body>
    </Card>
  );
}
