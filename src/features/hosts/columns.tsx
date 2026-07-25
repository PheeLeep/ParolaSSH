import { createColumnHelper, type ColumnDef } from "@tanstack/react-table";
import { Button, Dropdown } from "react-bootstrap";
import { Ellipsis, Pencil, Plug, Power, SquareTerminal, Trash2, Unplug } from "lucide-react";
import { formatAbsolute, formatRelative } from "../../lib/format";
import { StatusBadge, StatusDot } from "./StatusIndicator";
import type { HostRow } from "./HostsProvider";
import { AUTH_METHOD_LABELS } from "./types";

const column = createColumnHelper<HostRow>();

export type HostRowActions = {
  onConnect: (host: HostRow) => void;
  onDisconnect: (host: HostRow) => void;
  onEdit: (host: HostRow) => void;
  onDelete: (host: HostRow) => void;
  onPower: (host: HostRow) => void;
  onTerminal: (host: HostRow) => void;
};

export function createHostColumns(actions: HostRowActions): ColumnDef<HostRow, any>[] {
  // Built as a plain const so the column helper's inference isn't widened to
  // `any` by the declared return type.
  const columns = [
    column.accessor("label", {
      header: "Name",
      meta: { title: "Name" },
      cell: (info) => (
        <div className="d-flex align-items-center gap-2">
          <StatusDot status={info.row.original.status} />
          <div className="d-flex flex-column">
            <span className="fw-semibold">{info.getValue()}</span>
            <span className="text-body-secondary small">
              {info.row.original.username}@{info.row.original.hostname}
            </span>
          </div>
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
            <span key={tag} className="tag-chip">
              {tag}
            </span>
          ))}
        </div>
      ),
    }),

    column.accessor("status", {
      header: "Status",
      meta: { title: "Status", width: "8rem" },
      cell: (info) => <StatusBadge status={info.getValue()} />,
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
        const connected = host.status === "online";

        return (
          <div className="d-flex justify-content-end gap-1">
            {connected ? (
              <Button
                size="sm"
                variant="outline-secondary"
                onClick={() => actions.onDisconnect(host)}
              >
                <Unplug aria-hidden="true" />
                Disconnect
              </Button>
            ) : (
              <Button size="sm" variant="primary" onClick={() => actions.onConnect(host)}>
                <Plug aria-hidden="true" />
                Connect
              </Button>
            )}

            <Dropdown align="end">
              <Dropdown.Toggle
                size="sm"
                variant="outline-secondary"
                className="no-caret"
                id={`host-actions-${host.id}`}
                aria-label={`More actions for ${host.label}`}
              >
                <Ellipsis aria-hidden="true" />
              </Dropdown.Toggle>
              <Dropdown.Menu>
                {/* Both need a live session, so they say why they are off. */}
                <Dropdown.Item
                  disabled={!connected}
                  title={connected ? undefined : "Connect first"}
                  onClick={() => actions.onTerminal(host)}
                >
                  <SquareTerminal className="icon-sm" aria-hidden="true" />
                  Open terminal
                </Dropdown.Item>
                <Dropdown.Item
                  disabled={!connected}
                  title={connected ? undefined : "Connect first"}
                  onClick={() => actions.onPower(host)}
                >
                  <Power className="icon-sm" aria-hidden="true" />
                  Power…
                </Dropdown.Item>
                <Dropdown.Divider />
                <Dropdown.Item onClick={() => actions.onEdit(host)}>
                  <Pencil className="icon-sm" aria-hidden="true" />
                  Edit
                </Dropdown.Item>
                <Dropdown.Divider />
                <Dropdown.Item
                  className="text-danger"
                  onClick={() => actions.onDelete(host)}
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

  return columns as ColumnDef<HostRow, any>[];
}
