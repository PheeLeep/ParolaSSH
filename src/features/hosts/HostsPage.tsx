import { useMemo, useState } from "react";
import { Button, Card, Stack } from "react-bootstrap";
import { Plug, Plus } from "lucide-react";
import { DataTable } from "../../components/DataTable";
import { createHostColumns } from "./columns";
import { useHosts } from "./HostsProvider";
import type { SshHost } from "./types";
import type { Navigate } from "../../navigation";

export function HostsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { hosts, onlineCount, connect, edit, remove } = useHosts();
  const [selected, setSelected] = useState<SshHost[]>([]);

  const columns = useMemo(
    () => createHostColumns({ onConnect: connect, onEdit: edit, onDelete: remove }),
    [connect, edit, remove],
  );

  return (
    <div className="page">
      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">All hosts</h1>
          <p className="text-body-secondary mb-0">
            {hosts.length} saved {hosts.length === 1 ? "host" : "hosts"} ·{" "}
            {onlineCount} online
          </p>
        </div>
      </header>

      <Card>
        <Card.Body>
          <DataTable
            data={hosts}
            columns={columns}
            getRowId={(host) => host.id}
            enableRowSelection
            onSelectionChange={setSelected}
            onRowActivate={(host) => onNavigate({ kind: "host", hostId: host.id })}
            searchPlaceholder="Search hosts, tags, users…"
            emptyMessage="No saved connections. Add one to get started."
            toolbarActions={
              <Stack direction="horizontal" gap={2}>
                <Button size="sm" variant="primary">
                  <Plus aria-hidden="true" />
                  New connection
                </Button>
                <Button
                  size="sm"
                  variant="outline-secondary"
                  disabled={selected.length === 0}
                  onClick={() => selected.forEach(connect)}
                >
                  <Plug aria-hidden="true" />
                  Connect selected
                </Button>
              </Stack>
            }
          />
        </Card.Body>
      </Card>
    </div>
  );
}
