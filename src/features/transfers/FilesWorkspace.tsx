import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button } from "react-bootstrap";
import { Laptop, PanelLeftClose, PanelLeftOpen, Server, ShieldAlert, ShieldOff } from "lucide-react";

import * as api from "../hosts/api";
import { errorMessage } from "../hosts/api";
import { useElevation } from "../hosts/ElevationProvider";
import { useHosts } from "../hosts/HostsProvider";
import { readShowLocalPane, writeShowLocalPane } from "../settings/preferences";
import * as toast from "../../lib/toast";
import * as transfers from "./transferStore";
import { FilesPane, type FilesPaneHandle } from "./FilesPane";
import { LocalPane } from "./LocalPane";

/** How long to wait after a transfer lands before reloading, so a burst of
 *  small files costs one listing rather than one each. */
const RELOAD_DEBOUNCE_MS = 400;

/** The Files tab: this computer beside the host, with "Run as sudo". */
export function FilesWorkspace({ hostId }: { hostId: string }) {
  const { getConnection, getHost } = useHosts();
  const requestElevation = useElevation();
  const connection = getConnection(hostId);

  const canSudo =
    connection !== undefined &&
    connection.os !== "windows" &&
    (connection.elevation.kind === "sudoPassword" ||
      connection.elevation.kind === "sudoNoPassword");

  const [showLocal, setShowLocal] = useState(readShowLocalPane);
  const [elevated, setElevated] = useState(false);
  const [sudoBusy, setSudoBusy] = useState(false);
  const [localDir, setLocalDir] = useState<string | null>(null);
  const [remoteDir, setRemoteDir] = useState<string | null>(null);
  const [localReload, setLocalReload] = useState(0);
  const [remoteReload, setRemoteReload] = useState(0);
  const remote = useRef<FilesPaneHandle>(null);

  // Reload whichever side a finished transfer landed on.
  useEffect(() => {
    let pending = new Set<number>();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let landed = { upload: false, download: false };

    const check = () => {
      const next = new Set<number>();
      for (const record of transfers.forHost(hostId)) {
        if (record.state === "queued" || record.state === "running") next.add(record.id);
        else if (record.state === "done" && pending.has(record.id)) landed[record.direction] = true;
      }
      pending = next;
      if ((landed.upload || landed.download) && timer === undefined) {
        timer = setTimeout(() => {
          if (landed.upload) setRemoteReload((key) => key + 1);
          if (landed.download) setLocalReload((key) => key + 1);
          landed = { upload: false, download: false };
          timer = undefined;
        }, RELOAD_DEBOUNCE_MS);
      }
    };

    check();
    const unsubscribe = transfers.subscribe(check);
    return () => {
      unsubscribe();
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [hostId]);

  // Leaving the tab closes the root session rather than leaving it open unseen.
  const elevatedRef = useRef(elevated);
  elevatedRef.current = elevated;
  useEffect(
    () => () => {
      if (elevatedRef.current) void api.disableElevatedFiles(hostId).catch(() => undefined);
    },
    [hostId],
  );

  const toggleLocal = () => {
    setShowLocal((current) => {
      writeShowLocalPane(!current);
      return !current;
    });
    setLocalDir(null);
  };

  const toggleSudo = async () => {
    if (elevated) {
      setSudoBusy(true);
      await api.disableElevatedFiles(hostId).catch(() => undefined);
      setElevated(false);
      setSudoBusy(false);
      return;
    }

    const grant = await requestElevation({
      hostId,
      summary: "Browse and transfer files as root",
      command: "sudo sftp-server",
    });
    if (grant.outcome !== "granted") return;

    setSudoBusy(true);
    try {
      await api.enableElevatedFiles(hostId, grant.password);
      setElevated(true);
    } catch (caught) {
      toast.error("Could not run the file browser as root", errorMessage(caught));
    } finally {
      setSudoBusy(false);
    }
  };

  const uploadFrom = useCallback(
    async (...args: Parameters<FilesPaneHandle["uploadFrom"]>) => {
      await remote.current?.uploadFrom(...args);
    },
    [],
  );

  return (
    <div className={`files-split${showLocal ? "" : " files-split--single"}`}>
      {showLocal && (
        <section className="files-split__side" aria-label="This computer">
          <header className="files-split__title">
            <Laptop className="icon-sm" aria-hidden="true" />
            This computer
          </header>
          <LocalPane
            canUpload={remoteDir !== null}
            uploadHint={remoteDir ? `Upload to ${remoteDir}` : "Open a folder on the host first"}
            onUpload={uploadFrom}
            reloadKey={localReload}
            onPathChange={setLocalDir}
          />
        </section>
      )}

      <section className="files-split__side" aria-label="Remote host">
        <header className="files-split__title">
          <Server className="icon-sm" aria-hidden="true" />
          {getHost(hostId)?.label ?? "Host"}
          {elevated && <span className="status-badge status-badge--warning">root</span>}
          <span className="me-auto" />
          {canSudo && (
            <Button
              size="sm"
              variant={elevated ? "warning" : "outline-secondary"}
              onClick={() => void toggleSudo()}
              disabled={sudoBusy}
              title={elevated ? "Go back to browsing as your own account" : "Browse and transfer as root"}
            >
              {elevated ? (
                <ShieldOff className="icon-sm" aria-hidden="true" />
              ) : (
                <ShieldAlert className="icon-sm" aria-hidden="true" />
              )}
              {elevated ? "Stop sudo" : "Run as sudo"}
            </Button>
          )}
          <Button
            size="sm"
            variant="outline-secondary"
            onClick={toggleLocal}
            title={showLocal ? "Hide this computer" : "Show this computer beside the host"}
            aria-label={showLocal ? "Hide local files" : "Show local files"}
          >
            {showLocal ? (
              <PanelLeftClose className="icon-sm" aria-hidden="true" />
            ) : (
              <PanelLeftOpen className="icon-sm" aria-hidden="true" />
            )}
          </Button>
        </header>

        {elevated && (
          <Alert variant="warning" className="mb-0 py-2 small">
            Browsing as root. Uploads, new folders and edits are made as root,
            and files you upload will be owned by root.
          </Alert>
        )}

        <FilesPane
          ref={remote}
          hostId={hostId}
          elevated={elevated}
          localDir={showLocal ? localDir : null}
          reloadKey={remoteReload}
          onPathChange={setRemoteDir}
        />
      </section>
    </div>
  );
}
