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

  const [report, setReport] = useState<RemoteAuditReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A report from one host must never linger under another's audit tab.
  useEffect(() => {
    setReport(null);
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
    setReport((previous) => {
      if (!previous) return previous;
      const findings = previous.findings.map((entry) =>
        entry.id === finding.id ? { ...entry, suppressed: !entry.suppressed } : entry,
      );
      return { ...previous, findings, counts: count(findings), score: score(findings) };
    });
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

      {/* Tier 2 — detection only, by decision. */}
      <Card>
        <Card.Body className="d-flex gap-2">
          <Info className="icon-sm flex-shrink-0 mt-1 text-body-secondary" aria-hidden="true" />
          <div className="text-body-secondary small">
            {report?.lynis ? (
              <>
                <span className="fw-semibold text-body">{report.lynis}</span> is
                installed on this host. Running a full Lynis audit (minutes, pegs
                a core) from this pane is planned but not built yet — run{" "}
                <code>sudo lynis audit system</code> in the terminal if you want
                one now.
              </>
            ) : report ? (
              <>
                Lynis is not installed on this host, which is the normal case.
                This app will never install it for you; the checks above are the
                built-in tier.
              </>
            ) : (
              <>
                The deep scan tier uses Lynis when the host already has it.
                Running the checks above also detects whether it is installed.
              </>
            )}
          </div>
        </Card.Body>
      </Card>
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
