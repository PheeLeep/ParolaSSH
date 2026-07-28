import { useEffect, useState } from "react";
import { Alert, Button, Card, Spinner } from "react-bootstrap";
import {
  Bell,
  BellOff,
  Check,
  Copy,
  Info,
  Lock,
  ShieldCheck,
  Terminal,
} from "lucide-react";
import * as api from "../api";
import { errorMessage } from "../api";
import { useElevation } from "../ElevationProvider";
import { useHosts } from "../HostsProvider";
import * as auditCache from "../auditCache";
import { readAutoAudit } from "../../settings/preferences";
import { useStoreSubscription } from "../../../lib/useStoreSubscription";
import { SeverityBadge } from "../../keys/KeyIndicators";
import type {
  RemoteAuditReport,
  RemoteFinding,
  RemoteSeverity,
  RemoteSeverityCounts,
} from "../types";

const SEVERITY_RANK: Record<RemoteSeverity, number> = {
  critical: 4,
  high: 3,
  medium: 2,
  low: 1,
  info: 0,
};

/** The elevated half of the checks, shown in the prompt before it is agreed
 *  to. A copy of `TIER1_PRIVILEGED_COMMAND` in `remote/audit.rs`: shortening
 *  it for display would make the prompt describe something other than what
 *  runs, which is the whole thing this app refuses to do. */
const PRIVILEGED_COMMAND =
  `sudo -S -p '' sh -c 'sshd -T 2>&1 || /usr/sbin/sshd -T 2>&1; ` +
  `echo ---PAROLA:shadow---; awk -F: "(\\$2==\\"\\")" /etc/shadow | cut -d: -f1'`;

export function AuditPane({ hostId }: { hostId: string }) {
  const { getConnection } = useHosts();
  const requestElevation = useElevation();
  const connection = getConnection(hostId);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The report lives with the session, not with this component: the checks may
  // have run at connect time, long before this pane was opened. Keyed by host,
  // so one machine's report can never linger under another's tab.
  useStoreSubscription(auditCache.subscribe);
  const report = auditCache.get(hostId) ?? null;
  const setReport = (fresh: RemoteAuditReport) => auditCache.set(hostId, fresh);

  useEffect(() => {
    setError(null);
  }, [hostId]);

  const negotiated = connection?.negotiated ?? null;

  // Two of the checks need root. Where there is a route to it, the user is
  // asked before it is taken — and declining still runs the rest, because a
  // partial report is worth more than none.
  const canElevate =
    connection !== undefined &&
    connection.elevation.kind !== "unavailable" &&
    connection.os !== "windows";

  // The connect-time run is the main path (see `autoAudit` in HostsProvider).
  // This is the catch-up for the host that was already connected when the
  // preference was switched on — same unprivileged call, and `markAttempted`
  // means the two can never both fire.
  useEffect(() => {
    if (!readAutoAudit() || connection === undefined) return;
    if (!auditCache.markAttempted(hostId)) return;

    let cancelled = false;
    void (async () => {
      setBusy(true);
      try {
        const fresh = await api.remoteAudit(hostId, null, false);
        if (!cancelled) auditCache.set(hostId, fresh);
      } catch (caught) {
        if (!cancelled) setError(errorMessage(caught));
      } finally {
        if (!cancelled) setBusy(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [hostId, connection]);

  const run = async () => {
    let password: string | null = null;
    let elevate = false;

    if (canElevate) {
      const grant = await requestElevation({
        hostId,
        summary: "Run posture checks as root",
        command: PRIVILEGED_COMMAND,
        unprivilegedLabel: "Run without root",
      });
      if (grant.outcome === "cancelled") return;
      if (grant.outcome === "granted") {
        elevate = true;
        password = grant.password;
      }
    }

    setBusy(true);
    setError(null);
    try {
      setReport(await api.remoteAudit(hostId, password, elevate));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const toggleSuppressed = async (finding: RemoteFinding) => {
    await api.setRemoteFindingSuppressed(hostId, finding.id, !finding.suppressed);
    // Recomputed locally rather than re-running the audit: dismissing a
    // finding should not cost another round of remote commands.
    const previous = auditCache.get(hostId);
    if (!previous) return;

    const findings = previous.findings.map((entry) =>
      entry.id === finding.id ? { ...entry, suppressed: !entry.suppressed } : entry,
    );
    setReport({ ...previous, findings, counts: count(findings), score: score(findings) });
  };

  const sortedFindings = report
    ? [...report.findings].sort((a, b) => {
        if (a.suppressed !== b.suppressed) return a.suppressed ? 1 : -1;
        return SEVERITY_RANK[b.severity] - SEVERITY_RANK[a.severity];
      })
    : [];

  return (
    <div className="d-flex flex-column gap-3">
      {/* Tier 0 — free: the handshake already happened. */}
      <Card>
        <Card.Body>
          <div className="d-flex align-items-center gap-2 mb-2">
            <Lock className="icon-sm" aria-hidden="true" />
            <h2 className="h6 mb-0">Negotiated crypto</h2>
            <span className="text-body-secondary small">
              from the handshake — costs no remote command
            </span>
          </div>
          {negotiated ? (
            <dl className="detail-grid mb-0">
              <div>
                <dt>Key exchange</dt>
                <dd className="font-monospace small">{negotiated.kex}</dd>
              </div>
              <div>
                <dt>Host key</dt>
                <dd className="font-monospace small">{negotiated.hostKeyAlgorithm}</dd>
              </div>
              <div>
                <dt>Cipher</dt>
                <dd className="font-monospace small">{negotiated.cipher}</dd>
              </div>
              <div>
                <dt>MAC</dt>
                <dd className="font-monospace small">
                  {negotiated.clientMac === negotiated.serverMac
                    ? negotiated.clientMac
                    : `${negotiated.clientMac} / ${negotiated.serverMac}`}
                </dd>
              </div>
              <div>
                <dt>Strict key exchange</dt>
                <dd>
                  {negotiated.strictKex
                    ? "Supported (Terrapin mitigated)"
                    : "Not supported"}
                </dd>
              </div>
            </dl>
          ) : (
            <p className="text-body-secondary mb-0">
              The session did not report its negotiation — reconnect to refresh it.
            </p>
          )}
        </Card.Body>
      </Card>

      {/* Tier 1 sits behind an explicit button: it runs remote commands. */}
      {!report && (
        <Card>
          <Card.Body>
            <div className="d-flex align-items-center gap-2 mb-2">
              <Terminal className="icon-sm" aria-hidden="true" />
              <h2 className="h6 mb-0">Posture checks</h2>
            </div>
            <p className="text-body-secondary mb-3">
              A handful of read-only commands: <code>sshd -T</code> settings,{" "}
              <code>authorized_keys</code> permissions, world-writable PATH
              directories, and — where sudo allows — accounts with empty
              passwords. Nothing is changed on the host.
            </p>

            {error && <Alert variant="danger" className="text-prewrap">{error}</Alert>}

            <Button variant="primary" onClick={run} disabled={busy}>
              {busy ? (
                <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
              ) : (
                <ShieldCheck aria-hidden="true" />
              )}
              {busy ? "Checking…" : "Run checks"}
            </Button>
          </Card.Body>
        </Card>
      )}

      {report && (
        <>
          <div className="d-flex flex-wrap align-items-center gap-3">
            <div className="d-flex align-items-baseline gap-2">
              <span className="fs-3 fw-semibold">{report.score}</span>
              <span className="text-body-secondary">/ 100</span>
            </div>
            <CountsRow counts={report.counts} />
            <Button
              size="sm"
              variant="outline-secondary"
              className="ms-auto"
              disabled={busy}
              onClick={run}
            >
              {busy ? "Checking…" : "Run again"}
            </Button>
          </div>

          {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

          {report.tier1Note && (
            <Alert variant="secondary" className="d-flex gap-2 mb-0">
              <Info className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
              <div>{report.tier1Note}</div>
            </Alert>
          )}

          {sortedFindings.length === 0 ? (
            <Alert variant="success" className="mb-0">
              Nothing to report — every check that ran came back clean.
            </Alert>
          ) : (
            <div className="d-flex flex-column gap-2">
              {sortedFindings.map((finding) => (
                <RemoteFindingCard
                  key={finding.id}
                  finding={finding}
                  onToggleSuppressed={() => void toggleSuppressed(finding)}
                />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

function CountsRow({ counts }: { counts: RemoteSeverityCounts }) {
  const entries: Array<[RemoteSeverity, number]> = [
    ["critical", counts.critical],
    ["high", counts.high],
    ["medium", counts.medium],
    ["low", counts.low],
    ["info", counts.info],
  ];
  const present = entries.filter(([, value]) => value > 0);
  if (present.length === 0) {
    return <span className="text-body-secondary small">no open findings</span>;
  }
  return (
    <div className="d-flex flex-wrap gap-2">
      {present.map(([severity, value]) => (
        <span key={severity} className="d-inline-flex align-items-center gap-1 small">
          <SeverityBadge severity={severity} />
          <span className="text-body-secondary">{value}</span>
        </span>
      ))}
    </div>
  );
}

/** Like the key audit's FindingCard, minus local fixes: remote remediation
 *  is instruction text only — shown and copyable, never executed. */
function RemoteFindingCard({
  finding,
  onToggleSuppressed,
}: {
  finding: RemoteFinding;
  onToggleSuppressed: () => void;
}) {
  const [copied, setCopied] = useState(false);

  const copyInstruction = async (instruction: string) => {
    await navigator.clipboard.writeText(instruction);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <article
      className={`finding finding--${finding.severity}${
        finding.suppressed ? " is-suppressed" : ""
      }`}
    >
      <div className="finding__head">
        <SeverityBadge severity={finding.severity} />
        <h3 className="finding__title">{finding.title}</h3>
        <code className="finding__location">{finding.location}</code>
      </div>

      <p className="finding__detail">{finding.detail}</p>

      <div className="finding__actions">
        {finding.instruction && (
          <div className="finding__instruction">
            <code>{finding.instruction}</code>
            <Button
              size="sm"
              variant="link"
              className="p-0 text-decoration-none"
              onClick={() => void copyInstruction(finding.instruction!)}
              aria-label="Copy command"
            >
              {copied ? (
                <Check className="icon-sm" aria-hidden="true" />
              ) : (
                <Copy className="icon-sm" aria-hidden="true" />
              )}
            </Button>
          </div>
        )}

        <Button
          size="sm"
          variant="link"
          className="ms-auto p-0 text-decoration-none text-body-secondary"
          onClick={onToggleSuppressed}
        >
          {finding.suppressed ? (
            <>
              <Bell className="icon-sm" aria-hidden="true" />
              Restore
            </>
          ) : (
            <>
              <BellOff className="icon-sm" aria-hidden="true" />
              Dismiss
            </>
          )}
        </Button>
      </div>
    </article>
  );
}

/* Mirrors the Rust scoring exactly, so a dismissal updates the number without
   re-running remote commands. One rule costs its weight once, half again for
   repeats, nothing beyond. */

const WEIGHTS: Record<RemoteSeverity, number> = {
  critical: 25,
  high: 12,
  medium: 6,
  low: 2,
  info: 0,
};

function count(findings: RemoteFinding[]): RemoteSeverityCounts {
  const counts: RemoteSeverityCounts = { critical: 0, high: 0, medium: 0, low: 0, info: 0 };
  for (const finding of findings) {
    if (!finding.suppressed) counts[finding.severity] += 1;
  }
  return counts;
}

function score(findings: RemoteFinding[]): number {
  const perRule = new Map<string, { severity: RemoteSeverity; count: number }>();
  for (const finding of findings) {
    if (finding.suppressed) continue;
    const entry = perRule.get(finding.ruleId) ?? { severity: finding.severity, count: 0 };
    if (SEVERITY_RANK[finding.severity] > SEVERITY_RANK[entry.severity]) {
      entry.severity = finding.severity;
    }
    entry.count += 1;
    perRule.set(finding.ruleId, entry);
  }

  let penalty = 0;
  for (const { severity, count: instances } of perRule.values()) {
    const weight = WEIGHTS[severity];
    penalty += instances <= 1 ? weight : Math.floor((weight * 3) / 2);
  }
  return Math.max(0, 100 - penalty);
}
