import { createColumnHelper, type ColumnDef } from "@tanstack/react-table";
import { Badge, Button, Dropdown } from "react-bootstrap";
import { formatAbsolute, formatRelative } from "../../lib/format";
import {
  AUTH_METHOD_LABELS,
  STATUS_LABELS,
  type HostStatus,
  type SshHost,
} from "./types";

const column = createColumnHelper<SshHost>();

const STATUS_VARIANT: Record<HostStatus, string> = {
  online: "success",
  offline: "danger",
  unknown: "secondary",
};

export type HostRowActions = {
  onConnect: (host: SshHost) => void;
  onEdit: (host: SshHost) => void;
  onDelete: (host: SshHost) => void;
};

export function createHostColumns(actions: HostRowActions): ColumnDef<SshHost, any>[] {
  // Built as a plain const so the column helper's inference isn't widened to
  // `any` by the declared return type.
  const columns = [
    column.accessor("label", {
      header: "Name",
      meta: { title: "Name" },
      cell: (info) => (
        <div className="d-flex flex-column">
          <span className="fw-semibold">{info.getValue()}</span>
          <span className="text-body-secondary small">
            {info.row.original.username}@{info.row.original.hostname}
          </span>
        </div>
      ),
    }),

    column.accessor("hostname", {
      header: "Hostname",
      meta: { title: "Hostname" },
      cell: (info) => <code className="small">{info.getValue()}</code>,
    }),

    column.accessor("port", {
      header: "Port",
      meta: { title: "Port", width: "6rem", headerClassName: "text-end", cellClassName: "text-end" },
      cell: (info) => <span className="font-monospace small">{info.getValue()}</span>,
    }),

    column.accessor("username", {
      header: "User",
      meta: { title: "User", width: "8rem" },
    }),

    column.accessor("authMethod", {
      header: "Auth",
      meta: { title: "Auth", width: "9rem" },
      cell: (info) => (
        <span className="text-body-secondary">{AUTH_METHOD_LABELS[info.getValue()]}</span>
      ),
    }),

    column.accessor("group", {
      header: "Group",
      meta: { title: "Group", width: "9rem" },
    }),

    // Accessor flattens the array so the global search can match a tag.
    column.accessor((host) => host.tags.join(" "), {
      id: "tags",
      header: "Tags",
      enableSorting: false,
      meta: { title: "Tags" },
      cell: (info) => (
        <div className="d-flex flex-wrap gap-1">
          {info.row.original.tags.map((tag) => (
            <Badge key={tag} bg="secondary-subtle" text="secondary-emphasis" pill>
              {tag}
            </Badge>
          ))}
        </div>
      ),
    }),

    column.accessor("status", {
      header: "Status",
      meta: { title: "Status", width: "8rem" },
      cell: (info) => {
        const status = info.getValue();
        return (
          <Badge bg={STATUS_VARIANT[status]} className="fw-normal">
            {STATUS_LABELS[status]}
          </Badge>
        );
      },
    }),

    // Sort on the raw timestamp, display a friendly string.
    column.accessor((host) => (host.lastConnected ? Date.parse(host.lastConnected) : 0), {
      id: "lastConnected",
      header: "Last connected",
      meta: { title: "Last connected", width: "11rem" },
      cell: (info) => (
        <span title={formatAbsolute(info.row.original.lastConnected)}>
          {formatRelative(info.row.original.lastConnected)}
        </span>
      ),
    }),

    column.display({
      id: "actions",
      header: "",
      enableHiding: false,
      meta: { width: "8.5rem", cellClassName: "text-end" },
      cell: (info) => {
        const host = info.row.original;
        return (
          <div className="d-flex justify-content-end gap-1">
            <Button size="sm" variant="primary" onClick={() => actions.onConnect(host)}>
              <i className="bi bi-plug-fill me-1" aria-hidden="true" />
              Connect
            </Button>
            <Dropdown align="end">
              <Dropdown.Toggle
                size="sm"
                variant="outline-secondary"
                id={`host-actions-${host.id}`}
                aria-label={`More actions for ${host.label}`}
              />
              <Dropdown.Menu>
                <Dropdown.Item onClick={() => actions.onEdit(host)}>
                  <i className="bi bi-pencil me-2" aria-hidden="true" />
                  Edit
                </Dropdown.Item>
                <Dropdown.Divider />
                <Dropdown.Item
                  className="text-danger"
                  onClick={() => actions.onDelete(host)}
                >
                  <i className="bi bi-trash me-2" aria-hidden="true" />
                  Delete
                </Dropdown.Item>
              </Dropdown.Menu>
            </Dropdown>
          </div>
        );
      },
    }),
  ];

  return columns as ColumnDef<SshHost, any>[];
}
