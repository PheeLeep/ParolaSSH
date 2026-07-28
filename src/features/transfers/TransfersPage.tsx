import { useEffect, useSyncExternalStore } from "react";
import { Badge, Button, Card, Form, ProgressBar, Table } from "react-bootstrap";
import { Download, Eraser, FolderOpen, Upload, X } from "lucide-react";

import type { Navigate } from "../../navigation";
import type { TransferPriority, TransferRecord } from "../hosts/types";
import * as transfers from "./transferStore";
import { formatBytes, formatSpeed, percentOf } from "./format";

/**
 * Every transfer in the app, whichever host started it.
 *
 * Transfers outlive the pane that started them - you queue a download in one
 * host's Files tab and then go somewhere else - so this is the only place that
 * shows all of them at once, and it keeps updating no matter where the bytes
 * are coming from.
 *
 * Ordering here is display order (newest first). The queue's *running* order
 * lives in Rust and is shown per row as a priority and a position, so what the
 * scheduler will do next is legible without this page having to model it.
 */
export function TransfersPage({ onNavigate }: { onNavigate: Navigate }) {
  useSyncExternalStore(transfers.subscribe, transfers.getVersion);

  useEffect(() => {
    transfers.start();
    void transfers.refresh();
  }, []);

  const rows = transfers.all();
  const summary = transfers.counts();
  const finished = rows.filter((row) => !isPending(row)).length;

  return (
    <div className="page">
      <header className="d-flex flex-wrap align-items-center gap-3 mb-4">
        <div className="me-auto">
          <h1 className="page-title">Transfers</h1>
          <p className="text-body-secondary mb-0">
            {summary.running === 0 && summary.queued === 0
              ? "Nothing in flight"
              : `${summary.running} of ${summary.maxConcurrent} ${
                  summary.maxConcurrent === 1 ? "slot" : "slots"
                } running · ${summary.queued} queued`}
          </p>
        </div>

        {finished > 0 && (
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={() => void transfers.clearFinished()}
          >
            <Eraser className="icon-sm" aria-hidden="true" />
            Clear finished
          </Button>
        )}
      </header>

      {rows.length === 0 ? (
        <Card>
          <Card.Body className="text-center py-5">
            <FolderOpen className="icon-xl text-body-secondary mb-3" aria-hidden="true" />
            <h2 className="h5">No transfers yet</h2>
            <p className="text-body-secondary mb-0">
              Open a host's Files tab to upload or download. Transfers keep
              running while you work elsewhere, and appear here.
            </p>
          </Card.Body>
        </Card>
      ) : (
        <Card>
          <Table size="sm" className="align-middle mb-0 transfers-table">
            <thead>
              <tr>
                <th></th>
                <th>File</th>
                <th>Host</th>
                <th>Priority</th>
                <th className="transfers-table__progress">Progress</th>
                <th className="text-end">Speed</th>
                <th className="text-end">Actions</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <TransferRow key={row.id} row={row} onNavigate={onNavigate} />
              ))}
            </tbody>
          </Table>
        </Card>
      )}
    </div>
  );
}

function TransferRow({
  row,
  onNavigate,
}: {
  row: TransferRecord;
  onNavigate: Navigate;
}) {
  const pending = isPending(row);

  return (
    <tr className={row.state === "failed" ? "table-danger" : undefined}>
      <td>
        {row.direction === "download" ? (
          <Download className="icon-sm text-primary" aria-label="Download" />
        ) : (
          <Upload className="icon-sm text-success" aria-label="Upload" />
        )}
      </td>

      <td>
        <div className="fw-medium">{row.name}</div>
        <div className="text-body-secondary small font-monospace text-truncate">
          {row.direction === "download" ? row.remotePath : row.localPath}
        </div>
        {row.error && <div className="text-danger small text-prewrap">{row.error}</div>}
      </td>

      <td>
        <Button
          variant="link"
          size="sm"
          className="p-0"
          onClick={() => onNavigate({ kind: "host", hostId: row.hostId, feature: "files" })}
        >
          {row.hostLabel}
        </Button>
      </td>

      <td>
        {/* Only a waiting transfer can be re-prioritised; a running one has
            already spent its slot, so the picker would decide nothing. */}
        {row.state === "queued" ? (
          <PrioritySelect row={row} />
        ) : (
          <PriorityBadge priority={row.priority} />
        )}
      </td>

      <td className="transfers-table__progress">
        <Progress row={row} />
      </td>

      <td className="text-end text-body-secondary small font-monospace text-nowrap">
        {row.state === "running" ? formatSpeed(transfers.rateOf(row.id)) : "-"}
      </td>

      <td className="text-end">
        <div className="d-inline-flex gap-1">
          {pending && (
            <Button
              size="sm"
              variant="outline-danger"
              title="Cancel"
              onClick={() => void transfers.cancel(row.id)}
            >
              <X className="icon-sm" aria-hidden="true" />
            </Button>
          )}
        </div>
      </td>
    </tr>
  );
}

function Progress({ row }: { row: TransferRecord }) {
  if (row.state === "queued") {
    return (
      <span className="text-body-secondary small">
        {row.queuePosition === null ? "Queued" : `#${row.queuePosition} in queue`}
      </span>
    );
  }

  if (row.state === "running") {
    const percent = percentOf(row.bytesDone, row.bytesTotal);
    return (
      <div className="transfer-progress">
        <ProgressBar
          now={percent ?? 100}
          // A server that sends no size still shows motion rather than an
          // empty bar that looks stalled.
          animated={percent === null}
          striped={percent === null}
          className="transfer-progress__bar"
        />
        <span className="text-body-secondary small font-monospace">
          {formatBytes(row.bytesDone)}
          {row.bytesTotal ? ` / ${formatBytes(row.bytesTotal)}` : ""}
        </span>
      </div>
    );
  }

  const label: Record<string, string> = {
    done: "Complete",
    failed: "Failed",
    canceled: "Cancelled",
  };
  const variant: Record<string, string> = {
    done: "success",
    failed: "danger",
    canceled: "secondary",
  };

  return (
    <span className="d-inline-flex align-items-center gap-2">
      <Badge bg={variant[row.state]}>{label[row.state]}</Badge>
      {row.state === "done" && (
        <span className="text-body-secondary small font-monospace">
          {formatBytes(row.bytesDone)}
        </span>
      )}
    </span>
  );
}

const PRIORITIES: { value: TransferPriority; label: string }[] = [
  { value: "high", label: "High" },
  { value: "normal", label: "Normal" },
  { value: "low", label: "Low" },
];

/** The queue reorders as soon as this changes - Rust renumbers and the list
 *  refreshes, so the new position shows up in the same tick. */
function PrioritySelect({ row }: { row: TransferRecord }) {
  return (
    <Form.Select
      size="sm"
      className="priority-select"
      value={row.priority}
      aria-label={`Priority for ${row.name}`}
      onChange={(event) =>
        void transfers.prioritize(row.id, event.target.value as TransferPriority)
      }
    >
      {PRIORITIES.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </Form.Select>
  );
}

function PriorityBadge({ priority }: { priority: TransferRecord["priority"] }) {
  const look: Record<string, { bg: string; label: string }> = {
    high: { bg: "warning", label: "High" },
    normal: { bg: "secondary", label: "Normal" },
    low: { bg: "light", label: "Low" },
  };
  const { bg, label } = look[priority];

  return (
    <Badge bg={bg} text={bg === "light" ? "dark" : undefined} className="priority-badge">
      {label}
    </Badge>
  );
}

function isPending(row: TransferRecord): boolean {
  return row.state === "queued" || row.state === "running";
}
