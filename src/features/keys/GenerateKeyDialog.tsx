import { useEffect, useMemo, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { Check, Copy, KeyRound, TriangleAlert } from "lucide-react";
import { errorMessage } from "./api";
import { useKeys } from "./KeysProvider";
import { KEY_ALGORITHMS, type GenerateOutcome } from "./types";

const DEFAULT_NAMES: Record<string, string> = {
  ed25519: "id_ed25519",
  rsa: "id_rsa",
  ecdsa: "id_ecdsa",
};

export function GenerateKeyDialog({
  show,
  onClose,
}: {
  show: boolean;
  onClose: () => void;
}) {
  const { generate } = useKeys();

  const [algorithm, setAlgorithm] = useState<string>("ed25519");
  const [bits, setBits] = useState<number | null>(null);
  const [fileName, setFileName] = useState(DEFAULT_NAMES.ed25519);
  const [comment, setComment] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [overwrite, setOverwrite] = useState(false);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<GenerateOutcome | null>(null);
  const [copied, setCopied] = useState(false);

  const selected = useMemo(
    () => KEY_ALGORITHMS.find((option) => option.id === algorithm) ?? KEY_ALGORITHMS[0],
    [algorithm],
  );

  // Reset everything when the dialog is dismissed so a passphrase never
  // lingers in component state between openings.
  useEffect(() => {
    if (show) return;
    setAlgorithm("ed25519");
    setBits(null);
    setFileName(DEFAULT_NAMES.ed25519);
    setComment("");
    setPassphrase("");
    setConfirmation("");
    setOverwrite(false);
    setError(null);
    setOutcome(null);
    setCopied(false);
    setBusy(false);
  }, [show]);

  const chooseAlgorithm = (next: string) => {
    setAlgorithm(next);
    setFileName(DEFAULT_NAMES[next] ?? "id_key");
    const sizes = KEY_ALGORITHMS.find((option) => option.id === next)?.sizes;
    setBits(sizes ? sizes[sizes.length - 1] : null);
  };

  const mismatch = passphrase.length > 0 && confirmation !== passphrase;
  const canSubmit = fileName.trim().length > 0 && !mismatch && !busy;

  const submit = async () => {
    setBusy(true);
    setError(null);

    try {
      const result = await generate({
        algorithm,
        bits,
        fileName: fileName.trim(),
        comment: comment.trim() || null,
        passphrase: passphrase || null,
        overwrite,
      });
      setOutcome(result);
      // Clear the passphrase as soon as it has been used.
      setPassphrase("");
      setConfirmation("");
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  const copyPublicKey = async () => {
    if (!outcome) return;
    await navigator.clipboard.writeText(outcome.publicKeyOpenssh);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <Modal show={show} onHide={onClose} centered size="lg" backdrop="static">
      <Modal.Header closeButton>
        <Modal.Title className="d-flex align-items-center gap-2">
          <KeyRound aria-hidden="true" />
          {outcome ? "Key created" : "New SSH key"}
        </Modal.Title>
      </Modal.Header>

      {outcome ? (
        <>
          <Modal.Body>
            <p className="text-body-secondary">
              Written to <code>{outcome.privateKeyPath}</code> with owner-only
              permissions. Copy the public key below onto any server you want to
              reach.
            </p>

            <Form.Label className="fw-semibold">Public key</Form.Label>
            <div className="public-key-box user-select-auto">
              {outcome.publicKeyOpenssh}
            </div>

            <Button
              variant="outline-secondary"
              size="sm"
              className="mt-2"
              onClick={copyPublicKey}
            >
              {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
              {copied ? "Copied" : "Copy public key"}
            </Button>

            {!outcome.key.encrypted && (
              <Alert variant="warning" className="mt-3 mb-0 d-flex gap-2">
                <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
                <div>
                  This key has no passphrase. Anyone who copies the file can use
                  it. The audit will keep reminding you until you add one or
                  dismiss the finding.
                </div>
              </Alert>
            )}
          </Modal.Body>
          <Modal.Footer>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </Modal.Footer>
        </>
      ) : (
        <>
          <Modal.Body>
            {error && <Alert variant="danger">{error}</Alert>}

            <Form.Group className="mb-3">
              <Form.Label className="fw-semibold">Algorithm</Form.Label>
              <div className="algorithm-choices">
                {KEY_ALGORITHMS.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    className={`algorithm-choice${
                      algorithm === option.id ? " is-active" : ""
                    }`}
                    onClick={() => chooseAlgorithm(option.id)}
                    aria-pressed={algorithm === option.id}
                  >
                    <span className="algorithm-choice__name">{option.label}</span>
                    <span className="algorithm-choice__hint">{option.hint}</span>
                  </button>
                ))}
              </div>
            </Form.Group>

            {selected.sizes && (
              <Form.Group className="mb-3">
                <Form.Label>
                  {selected.id === "ecdsa" ? "Curve" : "Key size"}
                </Form.Label>
                <Form.Select
                  value={bits ?? selected.sizes[selected.sizes.length - 1]}
                  onChange={(event) => setBits(Number(event.target.value))}
                  className="w-auto"
                >
                  {selected.sizes.map((size) => (
                    <option key={size} value={size}>
                      {selected.id === "ecdsa" ? `P-${size}` : `${size} bits`}
                    </option>
                  ))}
                </Form.Select>
              </Form.Group>
            )}

            <Form.Group className="mb-3">
              <Form.Label>File name</Form.Label>
              <Form.Control
                value={fileName}
                onChange={(event) => setFileName(event.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
              <Form.Text className="text-body-secondary">
                Saved in your SSH directory. The public key gets the same name
                plus <code>.pub</code>.
              </Form.Text>
            </Form.Group>

            <Form.Group className="mb-3">
              <Form.Label>Comment</Form.Label>
              <Form.Control
                value={comment}
                onChange={(event) => setComment(event.target.value)}
                placeholder="you@laptop"
                spellCheck={false}
                autoComplete="off"
              />
              <Form.Text className="text-body-secondary">
                Shown on the public key. Useful for telling keys apart later.
              </Form.Text>
            </Form.Group>

            <Form.Group className="mb-3">
              <Form.Label>Passphrase</Form.Label>
              <Form.Control
                type="password"
                value={passphrase}
                onChange={(event) => setPassphrase(event.target.value)}
                autoComplete="new-password"
              />
              <Form.Text className="text-body-secondary">
                Leave blank for no passphrase — convenient for agents and CI, but
                the key file alone is then enough to authenticate as you.
              </Form.Text>
            </Form.Group>

            {passphrase.length > 0 && (
              <Form.Group className="mb-3">
                <Form.Label>Confirm passphrase</Form.Label>
                <Form.Control
                  type="password"
                  value={confirmation}
                  onChange={(event) => setConfirmation(event.target.value)}
                  isInvalid={mismatch}
                  autoComplete="new-password"
                />
                <Form.Control.Feedback type="invalid">
                  The passphrases do not match.
                </Form.Control.Feedback>
              </Form.Group>
            )}

            <Form.Check
              type="checkbox"
              id="overwrite-existing"
              label="Replace an existing key with this name"
              checked={overwrite}
              onChange={(event) => setOverwrite(event.target.checked)}
            />
            {overwrite && (
              <Alert variant="danger" className="mt-2 mb-0 d-flex gap-2">
                <TriangleAlert className="icon-sm flex-shrink-0 mt-1" aria-hidden="true" />
                <div>
                  The existing key will be overwritten and cannot be recovered.
                  Anything that trusts it will stop working.
                </div>
              </Alert>
            )}
          </Modal.Body>

          <Modal.Footer>
            <Button variant="outline-secondary" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={submit} disabled={!canSubmit}>
              {busy && (
                <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />
              )}
              {busy ? "Generating…" : "Create key"}
            </Button>
          </Modal.Footer>
        </>
      )}
    </Modal>
  );
}
