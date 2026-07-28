import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button, Spinner, Table } from "react-bootstrap";
import { CircleCheck, Info, PackageX, RefreshCw, ShieldAlert } from "lucide-react";
import * as api from "../api";
import { errorMessage } from "../api";
import type { UpdateReport } from "../types";

/** Read-only by design: the pane reports, the operator installs - in a
 *  terminal, on purpose. There is no install command to call. */
export function UpdatesPane({ hostId }: { hostId: string }) {
  const [report, setReport] = useState<UpdateReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setReport(await api.checkUpdates(hostId));
    } catch (caught) {
      setReport(null);
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="d-flex flex-column gap-3">
      <div className="d-flex align-items-center gap-2">
        <span className="text-body-secondary small me-auto">
          Read-only - nothing is ever installed from here.
        </span>
        <Button
          size="sm"
          variant="outline-secondary"
          disabled={loading}
          onClick={() => void refresh()}
        >
          <RefreshCw className="icon-sm" aria-hidden="true" />
          Check again
        </Button>
      </div>

      {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

      {loading ? (
        <div className="d-flex align-items-center gap-2 text-body-secondary py-4">
          <Spinner animation="border" size="sm" aria-hidden="true" />
          Asking the package manager… this can take a moment.
        </div>
      ) : report && <ReportBody report={report} />}
    </div>
  );
}

function ReportBody({ report }: { report: UpdateReport }) {
  switch (report.kind) {
    case "upToDate":
      return (
        <Alert variant="success" className="d-flex gap-2 mb-0">
          <CircleCheck className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
          <div>
            <span className="fw-semibold">{report.manager}</span> reports nothing
            pending - the host is up to date.
          </div>
        </Alert>
      );

    case "managerMissing":
      return (
        <Alert variant="secondary" className="d-flex gap-2 mb-0">
          <PackageX className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
          <div>{report.detail}</div>
        </Alert>
      );

    case "moduleMissing":
      return (
        <>
          <Alert variant="secondary" className="d-flex gap-2">
            <Info className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
            <div>{report.detail}</div>
          </Alert>
          {report.installedHistory.length > 0 && (
            <div>
              <h2 className="h6 mb-2">Recently installed updates</h2>
              <Table size="sm" className="align-middle mb-0">
                <thead>
                  <tr>
                    <th style={{ width: "10rem" }}>Update</th>
                    <th>Description</th>
                    <th style={{ width: "9rem" }}>Installed</th>
                  </tr>
                </thead>
                <tbody>
                  {report.installedHistory.map((hotfix) => (
                    <tr key={hotfix.id}>
                      <td className="font-monospace small">{hotfix.id}</td>
                      <td className="text-body-secondary small">{hotfix.description}</td>
                      <td className="small">{hotfix.installedOn ?? "-"}</td>
                    </tr>
                  ))}
                </tbody>
              </Table>
            </div>
          )}
        </>
      );

    case "list":
      return (
        <>
          <div className="d-flex align-items-center gap-2">
            <span className="fw-semibold">
              {report.updates.length}{" "}
              {report.updates.length === 1 ? "update" : "updates"} pending
            </span>
            <Badge bg="secondary">{report.manager}</Badge>
            {report.securityCount !== null && report.securityCount > 0 && (
              <span className="status-badge status-badge--offline">
                <ShieldAlert className="icon-sm" aria-hidden="true" />
                {report.securityCount} security
              </span>
            )}
            {report.securityCount === null && (
              <span className="text-body-secondary small">
                {report.manager} does not flag security updates per package.
              </span>
            )}
          </div>

          <Table size="sm" className="align-middle mb-0">
            <thead>
              <tr>
                <th>Package</th>
                <th>Version</th>
                <th style={{ width: "12rem" }}>Source</th>
                <th style={{ width: "7rem" }} />
              </tr>
            </thead>
            <tbody>
              {report.updates.map((update) => (
                <tr key={`${update.name}-${update.available}`}>
                  <td className="font-monospace small">{update.name}</td>
                  <td className="font-monospace small text-body-secondary">
                    {update.current ? `${update.current} → ` : ""}
                    <span className="text-body">{update.available}</span>
                  </td>
                  <td className="small text-body-secondary">{update.source}</td>
                  <td>
                    {update.security && (
                      <span className="status-badge status-badge--offline">Security</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        </>
      );
  }
}
