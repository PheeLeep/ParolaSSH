import { useState } from "react";
import { Button, ProgressBar } from "react-bootstrap";
import { Download, ExternalLink, X } from "lucide-react";
import { openExternal } from "../../lib/openExternal";
import { useStoreSubscription } from "../../lib/useStoreSubscription";
import { useHosts } from "../hosts/HostsProvider";
import * as updates from "./updateStore";

/** A strip above the panes when a newer ParolaSSH is out. */
export function UpdateBanner() {
  useStoreSubscription(updates.subscribe);
  const { connectedCount } = useHosts();
  const [confirming, setConfirming] = useState(false);
  const state = updates.getState();

  if (!updates.bannerVisible()) return null;

  if (state.status === "installing") {
    return (
      <div className="update-banner" role="status">
        <span>Installing ParolaSSH {state.version}…</span>
        {state.percent !== null && (
          <ProgressBar now={state.percent} className="update-banner__progress" aria-label="Download progress" />
        )}
      </div>
    );
  }
  if (state.status !== "available") return null;

  const install = () => {
    if (connectedCount > 0 && !confirming) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    void updates.installUpdate();
  };

  return (
    <div className="update-banner" role="status">
      <span>
        {confirming
          ? `Restarting closes ${connectedCount} open ${connectedCount === 1 ? "session" : "sessions"}.`
          : `ParolaSSH ${state.version} is available.`}
      </span>
      <span className="update-banner__actions">
        <Button size="sm" variant="link" className="p-0" onClick={() => void openExternal(updates.RELEASES_URL)}>
          What's new
          <ExternalLink className="icon-sm ms-1" aria-hidden="true" />
        </Button>
        {state.kind === "updatable" ? (
          <Button size="sm" variant="primary" onClick={install}>
            <Download className="icon-sm" aria-hidden="true" />
            {confirming ? "Install anyway" : "Install and restart"}
          </Button>
        ) : (
          <Button size="sm" variant="primary" onClick={() => void openExternal(updates.RELEASES_URL)}>
            <Download className="icon-sm" aria-hidden="true" />
            Download
          </Button>
        )}
        <Button
          size="sm"
          variant="link"
          className="p-0 text-body-secondary"
          aria-label="Later"
          onClick={() => {
            setConfirming(false);
            updates.dismiss();
          }}
        >
          <X className="icon-sm" aria-hidden="true" />
        </Button>
      </span>
    </div>
  );
}
