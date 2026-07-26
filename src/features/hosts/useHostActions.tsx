import { useCallback, useState, type ReactNode } from "react";
import { ConnectDialog } from "./ConnectDialog";
import { DeleteHostDialog } from "./DeleteHostDialog";
import { HostFormDialog } from "./HostFormDialog";
import { PowerDialog } from "./PowerDialog";
import { useHosts, type HostRow } from "./HostsProvider";
import { draftFromHost, emptyDraft, type HostDraft } from "./types";

/** The host dialogs, shared by the list and the detail pane, so a fix to the
 *  connect flow is made once. The caller renders `dialogs` anywhere — they are
 *  modals, so tree position does not matter. */
export function useHostActions(options: { onOpenTerminal?: (host: HostRow) => void } = {}) {
  const { connect, disconnect, getConnection } = useHosts();

  const [draft, setDraft] = useState<HostDraft | null>(null);
  const [connecting, setConnecting] = useState<HostRow | null>(null);
  const [deleting, setDeleting] = useState<HostRow | null>(null);
  const [powering, setPowering] = useState<HostRow | null>(null);

  const { onOpenTerminal } = options;

  const add = useCallback((group?: string) => setDraft(emptyDraft(group)), []);
  const edit = useCallback((host: HostRow) => setDraft(draftFromHost(host)), []);
  const remove = useCallback((host: HostRow) => setDeleting(host), []);
  const power = useCallback((host: HostRow) => setPowering(host), []);

  /** Agent auth still opens the dialog — it reports failures and host keys. */
  const startConnect = useCallback((host: HostRow) => setConnecting(host), []);

  const endConnect = useCallback(
    async (host: HostRow) => {
      await disconnect(host.id).catch(() => undefined);
    },
    [disconnect],
  );

  const openTerminal = useCallback(
    (host: HostRow) => {
      // A terminal needs a session; connect first and let the caller open the
      // pane once that succeeds.
      if (!getConnection(host.id)) {
        setConnecting(host);
        return;
      }
      onOpenTerminal?.(host);
    },
    [getConnection, onOpenTerminal],
  );

  const dialogs: ReactNode = (
    <>
      <HostFormDialog draft={draft} onClose={() => setDraft(null)} />
      <ConnectDialog host={connecting} onClose={() => setConnecting(null)} />
      <DeleteHostDialog host={deleting} onClose={() => setDeleting(null)} />
      <PowerDialog host={powering} onClose={() => setPowering(null)} />
    </>
  );

  return {
    actions: {
      onConnect: startConnect,
      onDisconnect: endConnect,
      onEdit: edit,
      onDelete: remove,
      onPower: power,
      onTerminal: openTerminal,
    },
    add,
    edit,
    connect,
    dialogs,
  };
}
