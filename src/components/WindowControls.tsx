import { useEffect, useState } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { appWindow } from "../lib/appWindow";

/**
 * Minimize / maximize / close for the undecorated window (Windows and Linux).
 *
 * Close goes through `close()`, not `destroy()`, so the CloseGuard gets to
 * intercept it while SSH sessions are live.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const sync = () => {
      void appWindow.isMaximized().then((value) => {
        if (!cancelled) setMaximized(value);
      });
    };
    sync();
    const unlisten = appWindow.onResized(sync);
    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className="window-controls">
      <button
        type="button"
        className="window-control"
        onClick={() => void appWindow.minimize()}
        aria-label="Minimize"
        title="Minimize"
      >
        <Minus aria-hidden="true" />
      </button>
      <button
        type="button"
        className="window-control"
        onClick={() => void appWindow.toggleMaximize()}
        aria-label={maximized ? "Restore" : "Maximize"}
        title={maximized ? "Restore" : "Maximize"}
      >
        {maximized ? <Copy aria-hidden="true" /> : <Square aria-hidden="true" />}
      </button>
      <button
        type="button"
        className="window-control window-control--close"
        onClick={() => void appWindow.close()}
        aria-label="Close"
        title="Close"
      >
        <X aria-hidden="true" />
      </button>
    </div>
  );
}
