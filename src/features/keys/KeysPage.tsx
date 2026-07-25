import { useMemo, useState } from "react";
import { Alert, Button, Card, Spinner, Stack } from "react-bootstrap";
import { FolderOpen, Plus, RefreshCw, ShieldCheck } from "lucide-react";
import { DataTable } from "../../components/DataTable";
import { createKeyColumns } from "./columns";
import { DeleteKeyDialog } from "./DeleteKeyDialog";
import { GenerateKeyDialog } from "./GenerateKeyDialog";
import { useKeys } from "./KeysProvider";
import { SEVERITY_ORDER, type Severity, type SshKey } from "./types";
import type { Navigate } from "../../navigation";

export function KeysPage({ onNavigate }: { onNavigate: Navigate }) {
  const { keys, location, report, loading, error, refresh, findingsForKey } = useKeys();
  const [showGenerate, setShowGenerate] = useState(false);
  const [keyToDelete, setKeyToDelete] = useState<SshKey | null>(null);

  const worstSeverity = useMemo(
    () => (key: SshKey): Severity | null => {
      const active = findingsForKey(key).filter((finding) => !finding.suppressed);
      // SEVERITY_ORDER runs worst-first, so the first hit wins.
      return SEVERITY_ORDER.find((severity) =>
        active.some((finding) => finding.severity === severity),
      ) ?? null;
    },
    [findingsForKey],
  );

  const columns = useMemo(
    () =>
      createKeyColumns({
        onOpen: (key) => onNavigate({ kind: "key", keyId: key.id }),
        onDelete: setKeyToDelete,
        worstSeverity,
      }),
    [onNavigate, worstSeverity],
  );

  const atRisk = keys.filter((key) => {
    const worst = worstSeverity(key);
    return worst === "critical" || worst === "high";
  }).length;

  return (
    <div className="page">
      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">SSH keys</h1>
          <p className="text-body-secondary mb-0">
            {loading && keys.length === 0 ? (
              "Scanning…"
            ) : (
              <>
                {keys.length} {keys.length === 1 ? "key" : "keys"}
                {atRisk > 0 && (
                  <>
                    {" · "}
                    <span className="text-danger fw-semibold">
                      {atRisk} needing attention
                    </span>
                  </>
                )}
                {location && (
                  <>
                    {" · "}
                    <code className="small">{location.path}</code>
                  </>
                )}
              </>
            )}
          </p>
        </div>

        <Stack direction="horizontal" gap={2}>
          <Button variant="primary" onClick={() => setShowGenerate(true)}>
            <Plus aria-hidden="true" />
            New key
          </Button>
          <Button
            variant="outline-secondary"
            onClick={() => onNavigate({ kind: "audit" })}
          >
            <ShieldCheck aria-hidden="true" />
            Security audit
            {report && report.counts.critical + report.counts.high > 0 && (
              <span className="count-pill count-pill--danger">
                {report.counts.critical + report.counts.high}
              </span>
            )}
          </Button>
          <Button
            variant="outline-secondary"
            onClick={() => void refresh()}
            disabled={loading}
            aria-label="Rescan"
          >
            <RefreshCw className={loading ? "spin" : undefined} aria-hidden="true" />
          </Button>
        </Stack>
      </header>

      {error && (
        <Alert variant="danger" className="d-flex align-items-center gap-2">
          <span className="flex-grow-1">{error}</span>
          <Button size="sm" variant="outline-danger" onClick={() => void refresh()}>
            Try again
          </Button>
        </Alert>
      )}

      {location && !location.exists && !loading && (
        <Alert variant="secondary" className="d-flex align-items-center gap-2">
          <FolderOpen className="icon-sm" aria-hidden="true" />
          <span className="flex-grow-1">
            No SSH directory at <code>{location.path}</code> yet. Creating a key
            will make one, locked to your account.
          </span>
        </Alert>
      )}

      {loading && keys.length === 0 ? (
        <div className="text-center text-body-secondary py-5">
          <Spinner animation="border" aria-hidden="true" />
          <div className="mt-2">Reading your SSH directory…</div>
        </div>
      ) : (
        <Card>
          <Card.Body>
            <DataTable
              data={keys}
              columns={columns}
              getRowId={(key) => key.id}
              onRowActivate={(key) => onNavigate({ kind: "key", keyId: key.id })}
              searchPlaceholder="Search keys, fingerprints, comments…"
              emptyMessage="No SSH keys found. Create one to get started."
            />
          </Card.Body>
        </Card>
      )}

      <GenerateKeyDialog show={showGenerate} onClose={() => setShowGenerate(false)} />

      <DeleteKeyDialog
        keyToDelete={keyToDelete}
        onClose={() => setKeyToDelete(null)}
      />
    </div>
  );
}
