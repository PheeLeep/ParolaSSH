import { createColumnHelper, type ColumnDef } from "@tanstack/react-table";
import { Button, Dropdown } from "react-bootstrap";
import { Ellipsis, KeyRound, ShieldAlert, Trash2 } from "lucide-react";
import { formatAbsolute, formatRelative } from "../../lib/format";
import {
  EncryptionBadge,
  PairingBadge,
  PermissionsBadge,
  SeverityDot,
} from "./KeyIndicators";
import {
  KEY_FORMAT_LABELS,
  describePairing,
  toIso,
  type Severity,
  type SshKey,
} from "./types";

const column = createColumnHelper<SshKey>();

export type KeyRowActions = {
  onOpen: (key: SshKey) => void;
  onDelete: (key: SshKey) => void;
  /** Worst severity among that key's findings, or null when it is clean. */
  worstSeverity: (key: SshKey) => Severity | null;
};

export function createKeyColumns(actions: KeyRowActions): ColumnDef<SshKey, any>[] {
  const columns = [
    column.accessor("fileName", {
      header: "Key",
      meta: { title: "Key" },
      cell: (info) => {
        const key = info.row.original;
        const worst = actions.worstSeverity(key);

        return (
          <div className="d-flex align-items-center gap-2">
            {worst ? <SeverityDot severity={worst} /> : <KeyRound className="icon-sm text-body-secondary" aria-hidden="true" />}
            <div className="d-flex flex-column">
              <span className="fw-semibold">{info.getValue()}</span>
              <span className="text-body-secondary small">
                {key.comment ?? key.path}
              </span>
            </div>
          </div>
        );
      },
    }),

    column.accessor("algorithm", {
      header: "Algorithm",
      meta: { title: "Algorithm", width: "10rem" },
      cell: (info) => <span className="font-monospace small">{info.getValue()}</span>,
    }),

    // Sort on the raw fingerprint, display it truncated by CSS.
    column.accessor((key) => key.fingerprint ?? "", {
      id: "fingerprint",
      header: "Fingerprint",
      meta: { title: "Fingerprint", width: "14rem" },
      cell: (info) => {
        const value = info.getValue() as string;
        return value ? (
          <code className="fingerprint small" title={value}>
            {value}
          </code>
        ) : (
          <span className="text-body-secondary">-</span>
        );
      },
    }),

    column.accessor("encrypted", {
      header: "Passphrase",
      meta: { title: "Passphrase", width: "10rem" },
      cell: (info) => <EncryptionBadge encrypted={info.getValue()} />,
    }),

    column.accessor((key) => key.permissions, {
      id: "permissions",
      header: "Permissions",
      enableSorting: false,
      meta: { title: "Permissions", width: "9rem" },
      cell: (info) => <PermissionsBadge permissions={info.row.original.permissions} />,
    }),

    // Sort on the verdict so mismatches group together.
    column.accessor((key) => describePairing(key.pairing), {
      id: "pairing",
      header: "Public key",
      meta: { title: "Public key", width: "9rem" },
      cell: (info) => <PairingBadge pairing={info.row.original.pairing} />,
    }),

    column.accessor("format", {
      header: "Format",
      meta: { title: "Format", width: "9rem" },
      cell: (info) => (
        <span className="text-body-secondary small">
          {KEY_FORMAT_LABELS[info.getValue()]}
        </span>
      ),
    }),

    column.accessor((key) => key.modifiedMs ?? 0, {
      id: "modified",
      header: "Modified",
      meta: { title: "Modified", width: "11rem" },
      cell: (info) => {
        const iso = toIso(info.row.original.modifiedMs);
        return <span title={formatAbsolute(iso)}>{formatRelative(iso)}</span>;
      },
    }),

    column.display({
      id: "actions",
      header: "",
      enableHiding: false,
      meta: { width: "9.5rem", sticky: "right", cellClassName: "text-end" },
      cell: (info) => {
        const key = info.row.original;
        const worst = actions.worstSeverity(key);
        const urgent = worst === "critical" || worst === "high";

        return (
          <div className="d-flex justify-content-end gap-1">
            <Button
              size="sm"
              variant={urgent ? "outline-danger" : "outline-secondary"}
              onClick={() => actions.onOpen(key)}
            >
              {urgent && <ShieldAlert aria-hidden="true" />}
              Details
            </Button>
            <Dropdown align="end">
              <Dropdown.Toggle
                size="sm"
                variant="outline-secondary"
                className="no-caret"
                id={`key-actions-${key.id}`}
                aria-label={`More actions for ${key.fileName}`}
              >
                <Ellipsis aria-hidden="true" />
              </Dropdown.Toggle>
              {/* Fixed strategy keeps the open menu from adding to the table's
                  scrollable overflow. */}
              <Dropdown.Menu popperConfig={{ strategy: "fixed" }}>
                <Dropdown.Item
                  className="text-danger"
                  onClick={() => actions.onDelete(key)}
                >
                  <Trash2 className="icon-sm" aria-hidden="true" />
                  Delete
                </Dropdown.Item>
              </Dropdown.Menu>
            </Dropdown>
          </div>
        );
      },
    }),
  ];

  return columns as ColumnDef<SshKey, any>[];
}
