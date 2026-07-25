import { useCallback, useMemo, useState } from "react";
import { Button, Card, Stack } from "react-bootstrap";
import { DataTable } from "../../components/DataTable";
import { createHostColumns } from "./columns";
import { sampleHosts } from "./sampleHosts";
import type { SshHost } from "./types";

export function HostsPage() {
  const [hosts] = useState<SshHost[]>(sampleHosts);
  const [selected, setSelected] = useState<SshHost[]>([]);

  // TODO: replace with Tauri commands once the Rust SSH layer lands.
  const handleConnect = useCallback((host: SshHost) => {
    console.info("connect", host.id);
  }, []);
  const handleEdit = useCallback((host: SshHost) => {
    console.info("edit", host.id);
  }, []);
  const handleDelete = useCallback((host: SshHost) => {
    console.info("delete", host.id);
  }, []);

  const columns = useMemo(
    () =>
      createHostColumns({
        onConnect: handleConnect,
        onEdit: handleEdit,
        onDelete: handleDelete,
      }),
    [handleConnect, handleEdit, handleDelete],
  );

  return (
    <Card className="shadow-sm">
      <Card.Header className="d-flex flex-wrap align-items-center gap-2 bg-body-tertiary">
        <div>
          <h5 className="mb-0">Connections</h5>
          <small className="text-body-secondary">
            {hosts.length} saved {hosts.length === 1 ? "host" : "hosts"}
          </small>
        </div>
      </Card.Header>

      <Card.Body>
        <DataTable
          data={hosts}
          columns={columns}
          getRowId={(host) => host.id}
          enableRowSelection
          onSelectionChange={setSelected}
          onRowActivate={handleConnect}
          searchPlaceholder="Search hosts, tags, users…"
          emptyMessage="No saved connections. Add one to get started."
          toolbarActions={
            <Stack direction="horizontal" gap={2}>
              <Button size="sm" variant="primary">
                <i className="bi bi-plus-lg me-1" aria-hidden="true" />
                New connection
              </Button>
              <Button
                size="sm"
                variant="outline-secondary"
                disabled={selected.length === 0}
              >
                <i className="bi bi-plug me-1" aria-hidden="true" />
                Connect selected
              </Button>
            </Stack>
          }
        />
      </Card.Body>
    </Card>
  );
}
