import { useMemo, useState } from "react";
import { Alert, Button, Form, Spinner, Stack } from "react-bootstrap";
import { ChevronLeft, Link2, RefreshCw, ShieldCheck } from "lucide-react";
import { FindingCard } from "./FindingCard";
import { SeverityIcon } from "./KeyIndicators";
import { useKeys } from "./KeysProvider";
import { formatRelative } from "../../lib/format";
import {
  SEVERITY_LABELS,
  SEVERITY_ORDER,
  describePermissions,
  toIso,
  type Severity,
} from "./types";
import type { Navigate } from "../../navigation";

export function AuditPage({ onNavigate }: { onNavigate: Navigate }) {
  const { report, loading, error, refresh } = useKeys();
  const [showSuppressed, setShowSuppressed] = useState(false);
  const [filter, setFilter] = useState<Severity | "all">("all");

  const visible = useMemo(() => {
    const findings = report?.findings ?? [];
    return findings
      .filter((finding) => showSuppressed || !finding.suppressed)
      .filter((finding) => filter === "all" || finding.severity === filter);
  }, [report, showSuppressed, filter]);

  const suppressedCount =
    report?.findings.filter((finding) => finding.suppressed).length ?? 0;

  if (loading && !report) {
    return (
      <div className="page text-center text-body-secondary py-5">
        <Spinner animation="border" aria-hidden="true" />
        <div className="mt-2">Auditing your SSH directory…</div>
      </div>
    );
  }

  return (
    <div className="page">
      <Button
        variant="link"
        size="sm"
        className="p-0 mb-3 text-decoration-none text-body-secondary"
        onClick={() => onNavigate({ kind: "keys" })}
      >
        <ChevronLeft className="icon-sm" aria-hidden="true" />
        All keys
      </Button>

      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">Security audit</h1>
          <p className="text-body-secondary mb-0">
            {report ? (
              <>
                <code className="small">{report.sshDir}</code>
                {" · "}
                {report.keyCount} {report.keyCount === 1 ? "key" : "keys"}
                {" · "}
                scanned {formatRelative(toIso(report.scannedAtMs)).toLowerCase()}
              </>
            ) : (
              "Not scanned yet."
            )}
          </p>
        </div>

        <Button
          variant="outline-secondary"
          onClick={() => void refresh()}
          disabled={loading}
        >
          <RefreshCw className={loading ? "spin" : undefined} aria-hidden="true" />
          Rescan
        </Button>
      </header>

      {error && <Alert variant="danger">{error}</Alert>}

      {report && !report.dirExists && (
        <Alert variant="secondary">
          There is no SSH directory at <code>{report.sshDir}</code> yet, so there
          is nothing to audit. Create a key and it will appear here.
        </Alert>
      )}

      {report && report.dirExists && (
        <>
          <div className="audit-summary mb-4">
            <ScoreTile score={report.score} />

            {SEVERITY_ORDER.map((severity) => (
              <button
                key={severity}
                type="button"
                className={`severity-tile severity-tile--${severity}${
                  filter === severity ? " is-active" : ""
                }${report.counts[severity] === 0 ? " is-empty" : ""}`}
                onClick={() => setFilter(filter === severity ? "all" : severity)}
                aria-pressed={filter === severity}
              >
                <SeverityIcon severity={severity} className="severity-tile__icon" />
                <span className="severity-tile__count">
                  {report.counts[severity]}
                </span>
                <span className="severity-tile__label">
                  {SEVERITY_LABELS[severity]}
                </span>
              </button>
            ))}
          </div>

          {report.symlinkTarget && (
            <Alert variant="info" className="d-flex gap-2">
              <Link2 className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
              <div>
                Your SSH directory is a symlink to{" "}
                <code>{report.symlinkTarget}</code>. Permissions are read from
                the target, and applying a fix here changes that file — check
                whether it is tracked in a dotfiles repository first.
              </div>
            </Alert>
          )}

          <div className="d-flex flex-wrap align-items-center gap-3 mb-3">
            <h2 className="section-title mb-0 me-auto">
              {filter === "all"
                ? "Findings"
                : `${SEVERITY_LABELS[filter]} findings`}
            </h2>

            <Stack direction="horizontal" gap={3}>
              {filter !== "all" && (
                <Button
                  variant="link"
                  size="sm"
                  className="p-0 text-decoration-none"
                  onClick={() => setFilter("all")}
                >
                  Show all severities
                </Button>
              )}

              {suppressedCount > 0 && (
                <Form.Check
                  type="switch"
                  id="show-suppressed"
                  label={`Show ${suppressedCount} dismissed`}
                  checked={showSuppressed}
                  onChange={(event) => setShowSuppressed(event.target.checked)}
                />
              )}
            </Stack>
          </div>

          <p className="text-body-secondary small mb-3">
            Directory permissions:{" "}
            <code>{describePermissions(report.directoryPermissions)}</code>
          </p>

          {visible.length === 0 ? (
            <Alert variant="success" className="d-flex align-items-center gap-2">
              <ShieldCheck className="icon-sm" aria-hidden="true" />
              {filter === "all"
                ? "Nothing outstanding. Your SSH directory looks healthy."
                : `No ${SEVERITY_LABELS[filter].toLowerCase()} findings.`}
            </Alert>
          ) : (
            <div className="finding-list">
              {visible.map((finding) => (
                <FindingCard key={finding.id} finding={finding} />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** The score is a summary, not a grade — the findings below are the substance. */
function ScoreTile({ score }: { score: number }) {
  const tone = score >= 90 ? "good" : score >= 70 ? "fair" : score >= 40 ? "poor" : "bad";

  return (
    <div className={`score-tile score-tile--${tone}`}>
      <span className="score-tile__value">{score}</span>
      <span className="score-tile__label">Score</span>
    </div>
  );
}
