import { useState } from "react";
import { Button, Spinner } from "react-bootstrap";
import { Bell, BellOff, Check, Copy, Wrench } from "lucide-react";
import { errorMessage } from "./api";
import { useKeys } from "./KeysProvider";
import { SeverityBadge } from "./KeyIndicators";
import type { Finding } from "./types";

/** One finding, with whatever action it supports.
 *
 *  Automatic fixes are per-finding and explicit - there is deliberately no
 *  "fix everything" button, because a mass permission change that guesses
 *  wrong (a symlinked dotfiles repo, say) is worse than the finding. */
export function FindingCard({ finding }: { finding: Finding }) {
  const { applyPermissionFix, setSuppressed } = useKeys();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const copyInstruction = async (instruction: string) => {
    await navigator.clipboard.writeText(instruction);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  // Pulled out so TypeScript narrows the union once, rather than at each use.
  const remediation = finding.remediation;

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

      {error && <p className="text-danger small mb-2">{error}</p>}

      <div className="finding__actions">
        {remediation?.kind === "restrictPermissions" && (
          <Button
            size="sm"
            variant="primary"
            disabled={busy}
            onClick={() =>
              run(() => applyPermissionFix(remediation.path, remediation.isDir))
            }
          >
            {busy ? (
              <Spinner animation="border" size="sm" aria-hidden="true" />
            ) : (
              <Wrench aria-hidden="true" />
            )}
            Restrict to me
          </Button>
        )}

        {remediation?.kind === "manual" && (
          <div className="finding__instruction">
            <code>{remediation.instruction}</code>
            <Button
              size="sm"
              variant="link"
              className="p-0 text-decoration-none"
              onClick={() => copyInstruction(remediation.instruction)}
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
          disabled={busy}
          onClick={() => run(() => setSuppressed(finding.id, !finding.suppressed))}
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
