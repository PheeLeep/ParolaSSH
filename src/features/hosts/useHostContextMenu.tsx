import { useCallback, useState, type MouseEvent, type ReactNode } from "react";
import {
  ExternalLink,
  Pencil,
  Plug,
  Power,
  SquareTerminal,
  Trash2,
  Unplug,
} from "lucide-react";
import { ContextMenu, type ContextMenuAnchor, type ContextMenuItem } from "../../components/ContextMenu";
import type { HostRowActions } from "./columns";
import type { HostRow } from "./HostsProvider";

/**
 * Right-click on a host, wherever it is listed. The entries are the same ones
 * the row's ⋯ menu offers, so a fix to the connect flow is still made once —
 * this only changes how they are reached.
 */
export function useHostContextMenu({
  actions,
  onOpen,
}: {
  actions: HostRowActions;
  onOpen?: (host: HostRow) => void;
}): { openContextMenu: (host: HostRow, event: MouseEvent) => void; contextMenu: ReactNode } {
  const [target, setTarget] = useState<
    { host: HostRow; anchor: ContextMenuAnchor } | null
  >(null);

  const openContextMenu = useCallback((host: HostRow, event: MouseEvent) => {
    event.preventDefault();
    setTarget({ host, anchor: { x: event.clientX, y: event.clientY } });
  }, []);

  const close = useCallback(() => setTarget(null), []);

  let contextMenu: ReactNode = null;
  if (target) {
    const { host } = target;
    const connected = host.status === "connected";
    // Both need a live session, so they say why they are off rather than
    // vanishing — a menu that changes shape is harder to learn.
    const needsSession = connected ? undefined : "Connect first";

    const items: ContextMenuItem[] = [
      connected
        ? {
            label: "Disconnect",
            Icon: Unplug,
            onSelect: () => actions.onDisconnect(host),
          }
        : {
            label: "Connect",
            Icon: Plug,
            onSelect: () => actions.onConnect(host),
          },
      {
        label: "Open terminal",
        Icon: SquareTerminal,
        disabled: !connected,
        title: needsSession,
        onSelect: () => actions.onTerminal(host),
      },
      {
        label: "Power…",
        Icon: Power,
        disabled: !connected,
        title: needsSession,
        onSelect: () => actions.onPower(host),
      },
      { separator: true },
      ...(onOpen
        ? [
            {
              label: "Open host",
              Icon: ExternalLink,
              onSelect: () => onOpen(host),
            } as ContextMenuItem,
            { separator: true } as ContextMenuItem,
          ]
        : []),
      { label: "Edit", Icon: Pencil, onSelect: () => actions.onEdit(host) },
      {
        label: "Delete",
        Icon: Trash2,
        danger: true,
        onSelect: () => actions.onDelete(host),
      },
    ];

    contextMenu = (
      <ContextMenu
        anchor={target.anchor}
        items={items}
        onClose={close}
        label={`Actions for ${host.label}`}
      />
    );
  }

  return { openContextMenu, contextMenu };
}
