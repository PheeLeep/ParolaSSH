import { useEffect, useState } from "react";
import { Alert, Button, Card, Spinner } from "react-bootstrap";
import { Check, ChevronLeft, Copy, ShieldCheck, Trash2 } from "lucide-react";
import { errorMessage, readPublicKey } from "./api";
import { DeleteKeyDialog } from "./DeleteKeyDialog";
import { FindingCard } from "./FindingCard";
import { EncryptionBadge, PairingBadge, PermissionsBadge } from "./KeyIndicators";
import { useKeys } from "./KeysProvider";
import { formatAbsolute, formatRelative } from "../../lib/format";
import {
  describeKdf,
  describePermissions,
  explainPairing,
  KEY_FORMAT_LABELS,
  SEVERITY_ORDER,
  toIso,
} from "./types";
import type { Navigate } from "../../navigation";

export function KeyDetail({
  keyId,
  onNavigate,
}: {
  keyId: string;
  onNavigate: Navigate;
}) {
  const { getKey, findingsForKey, loading } = useKeys();
  const key = getKey(keyId);

  const [publicKey, setPublicKey] = useState<string | null>(null);
  const [publicKeyError, setPublicKeyError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  const publicKeyPath = key?.publicKeyPath ?? null;
  const inlinePublicKey = key?.publicKeyOpenssh ?? null;

  useEffect(() => {
    setCopied(false);
    setPublicKeyError(null);

    // Prefer what the scan already parsed; only read the file when the key
    // itself could not be parsed (a legacy PEM, say).
    if (inlinePublicKey) {
      setPublicKey(inlinePublicKey);
      return;
    }
    if (!publicKeyPath) {
      setPublicKey(null);
      return;
    }

    let cancelled = false;
    readPublicKey(publicKeyPath)
      .then((text) => {
        if (!cancelled) setPublicKey(text);
      })
      .catch((caught) => {
        if (!cancelled) setPublicKeyError(errorMessage(caught));
      });

    return () => {
      cancelled = true;
    };
  }, [inlinePublicKey, publicKeyPath]);

  if (!key) {
    return (
      <div className="page">
        {loading ? (
          <div className="text-center text-body-secondary py-5">
            <Spinner animation="border" aria-hidden="true" />
          </div>
        ) : (
          <p className="text-body-secondary">
            That key is no longer there.{" "}
            <Button
              variant="link"
              className="p-0 align-baseline"
              onClick={() => onNavigate({ kind: "keys" })}
            >
              Back to all keys
            </Button>
          </p>
        )}
      </div>
    );
  }

  const findings = findingsForKey(key);
  const active = findings.filter((finding) => !finding.suppressed);
  const sorted = [...findings].sort(
    (a, b) =>
      SEVERITY_ORDER.indexOf(a.severity) - SEVERITY_ORDER.indexOf(b.severity),
  );

  const copyPublicKey = async () => {
    if (!publicKey) return;
    await navigator.clipboard.writeText(publicKey);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

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
          <div className="d-flex align-items-center gap-2 mb-1">
            <h1 className="page-title">{key.fileName}</h1>
            <EncryptionBadge encrypted={key.encrypted} />
            <PermissionsBadge permissions={key.permissions} />
          </div>
          <p className="text-body-secondary font-monospace small mb-0">{key.path}</p>
        </div>

        <Button variant="outline-danger" onClick={() => setConfirmingDelete(true)}>
          <Trash2 aria-hidden="true" />
          Delete
        </Button>
      </header>

      {active.length === 0 && (
        <Alert variant="success" className="d-flex align-items-center gap-2">
          <ShieldCheck className="icon-sm" aria-hidden="true" />
          Nothing outstanding for this key.
        </Alert>
      )}

      <Card className="mb-4">
        <Card.Body>
          <dl className="detail-grid mb-0">
            <div>
              <dt>Algorithm</dt>
              <dd className="font-monospace">{key.algorithm}</dd>
            </div>
            <div>
              <dt>Fingerprint</dt>
              <dd className="font-monospace text-break user-select-auto">
                {key.fingerprint ?? "—"}
              </dd>
            </div>
            <div>
              <dt>Comment</dt>
              <dd>{key.comment ?? <span className="text-body-secondary">None</span>}</dd>
            </div>
            <div>
              <dt>Format</dt>
              <dd>{KEY_FORMAT_LABELS[key.format]}</dd>
            </div>
            <div>
              <dt>Passphrase</dt>
              <dd>{describeKdf(key.kdf)}</dd>
            </div>
            <div>
              <dt>Permissions</dt>
              <dd className="font-monospace">{describePermissions(key.permissions)}</dd>
            </div>
            <div>
              <dt>Modified</dt>
              <dd title={formatAbsolute(toIso(key.modifiedMs))}>
                {formatRelative(toIso(key.modifiedMs))}
              </dd>
            </div>
            <div>
              <dt>Public key pairing</dt>
              <dd className="d-flex flex-column gap-1 align-items-start">
                <PairingBadge pairing={key.pairing} />
                <span className="text-body-secondary small">
                  {explainPairing(key.pairing)}
                </span>
              </dd>
            </div>
            <div>
              <dt>Public key file</dt>
              <dd className="font-monospace small text-break">
                {key.publicKeyPath ?? (
                  <span className="text-body-secondary font-monospace">None</span>
                )}
              </dd>
            </div>
          </dl>

          {key.parseError && (
            <Alert variant="warning" className="mt-3 mb-0">
              This file could not be fully parsed: {key.parseError}
            </Alert>
          )}
        </Card.Body>
      </Card>

      <div className="d-flex align-items-center gap-2 mb-3">
        <h2 className="section-title">Public key</h2>
        {publicKey && (
          <Button
            variant="link"
            size="sm"
            className="ms-auto p-0 text-decoration-none"
            onClick={copyPublicKey}
          >
            {copied ? <Check className="icon-sm" aria-hidden="true" /> : <Copy className="icon-sm" aria-hidden="true" />}
            {copied ? "Copied" : "Copy"}
          </Button>
        )}
      </div>

      {publicKey ? (
        <div className="public-key-box user-select-auto mb-4">{publicKey}</div>
      ) : (
        <p className="text-body-secondary mb-4">
          {publicKeyError ?? "No public key file alongside this key."}
        </p>
      )}

      <DeleteKeyDialog
        keyToDelete={confirmingDelete ? key : null}
        onClose={() => setConfirmingDelete(false)}
        // The key is gone, so this view has nothing left to show.
        onDeleted={() => onNavigate({ kind: "keys" })}
      />

      {sorted.length > 0 && (
        <>
          <h2 className="section-title mb-3">
            Findings
            {active.length > 0 && (
              <span className="count-pill count-pill--danger">{active.length}</span>
            )}
          </h2>
          <div className="finding-list">
            {sorted.map((finding) => (
              <FindingCard key={finding.id} finding={finding} />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
