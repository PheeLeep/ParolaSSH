import { useMemo, useState } from "react";
import { Alert, Button, Card, Spinner, Stack } from "react-bootstrap";
import { FileInput, Plug, Plus } from "lucide-react";
import { DataTable } from "../../components/DataTable";
import { createHostColumns } from "./columns";
import { useHosts, type HostRow } from "./HostsProvider";
import { useHostActions } from "./useHostActions";
import { useHostContextMenu } from "./useHostContextMenu";
import { SshConfigImportDialog } from "./SshConfigImportDialog";
import type { Navigate } from "../../navigation";

export function HostsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { hosts, connectedCount, loading, error } = useHosts();
  const [selected, setSelected] = useState<HostRow[]>([]);
  const [importing, setImporting] = useState(false);

  const { actions, add, dialogs } = useHostActions({
    // A terminal lives on the detail pane, so opening one navigates there.
    onOpenTerminal: (host) => onNavigate({ kind: "host", hostId: host.id }),
  });

  const { openContextMenu, contextMenu } = useHostContextMenu({
    actions,
    onOpen: (host) => onNavigate({ kind: "host", hostId: host.id }),
  });

  const columns = useMemo(() => createHostColumns(actions), [actions]);

  return (
    <div className="page">
      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">All hosts</h1>
          <p className="text-body-secondary mb-0">
            {hosts.length} saved {hosts.length === 1 ? "host" : "hosts"} ·{" "}
            {connectedCount} connected
          </p>
        </div>
      </header>

      {error && <Alert variant="danger">{error}</Alert>}

      <Card>
        <Card.Body>
          {loading && hosts.length === 0 ? (
            <div className="d-flex align-items-center gap-2 text-body-secondary py-4">
              <Spinner animation="border" size="sm" aria-hidden="true" />
              Loading saved connections…
            </div>
          ) : (
            <DataTable
              data={hosts}
              columns={columns}
              getRowId={(host) => host.id}
              enableRowSelection
              onSelectionChange={setSelected}
              onRowActivate={(host) => onNavigate({ kind: "host", hostId: host.id })}
              onRowContextMenu={openContextMenu}
              searchPlaceholder="Search hosts, tags, users…"
              emptyMessage="No saved connections. Add one to get started."
              toolbarActions={
                <Stack direction="horizontal" gap={2}>
                  <Button size="sm" variant="primary" onClick={() => add()}>
                    <Plus aria-hidden="true" />
                    New connection
                  </Button>
                  <Button
                    size="sm"
                    variant="outline-secondary"
                    onClick={() => setImporting(true)}
                  >
                    <FileInput aria-hidden="true" />
                    Import from ssh_config
                  </Button>
                  <Button
                    size="sm"
                    variant="outline-secondary"
                    // Each connection may need its own password, so this walks
                    // them one dialog at a time rather than firing in parallel.
                    disabled={selected.length !== 1}
                    title={
                      selected.length > 1
                        ? "Connect one at a time - each may need its own password"
                        : undefined
                    }
                    onClick={() => selected[0] && actions.onConnect(selected[0])}
                  >
                    <Plug aria-hidden="true" />
                    Connect selected
                  </Button>
                </Stack>
              }
            />
          )}
        </Card.Body>
      </Card>

      <SshConfigImportDialog
        show={importing}
        onClose={() => setImporting(false)}
      />

      {contextMenu}
      {dialogs}
    </div>
  );
}
