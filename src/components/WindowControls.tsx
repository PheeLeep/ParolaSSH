import { useEffect, useState } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { appWindow } from "../lib/appWindow";
import type { NavStyle } from "../features/settings/preferences";

/**
 * Minimize / maximize / close for the undecorated window (Windows and Linux),
 * drawn in the convention `style` asks for — see Settings › Navigation layout.
 *
 * Close goes through `close()`, not `destroy()`, so the CloseGuard gets to
 * intercept it while SSH sessions are live.
 */
export function WindowControls({ style }: { style: NavStyle }) {
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

  const minimize = () => void appWindow.minimize();
  const toggle = () => void appWindow.toggleMaximize();
  const close = () => void appWindow.close();
  const restoreLabel = maximized ? "Restore" : "Maximize";

  // Traffic lights: close first, and the glyphs only appear on hover, the way
  // the platform draws them.
  if (style === "macos") {
    return (
      <div className="window-controls window-controls--mac">
        <button
          type="button"
          className="traffic-light traffic-light--close"
          onClick={close}
          aria-label="Close"
          title="Close"
        >
          <X aria-hidden="true" />
        </button>
        <button
          type="button"
          className="traffic-light traffic-light--minimize"
          onClick={minimize}
          aria-label="Minimize"
          title="Minimize"
        >
          <Minus aria-hidden="true" />
        </button>
        <button
          type="button"
          className="traffic-light traffic-light--zoom"
          onClick={toggle}
          aria-label={restoreLabel}
          title={restoreLabel}
        >
          <Square aria-hidden="true" />
        </button>
      </div>
    );
  }

  return (
    <div className={`window-controls window-controls--${style}`}>
      <button
        type="button"
        className="window-control"
        onClick={minimize}
        aria-label="Minimize"
        title="Minimize"
      >
        <Minus aria-hidden="true" />
      </button>
      <button
        type="button"
        className="window-control"
        onClick={toggle}
        aria-label={restoreLabel}
        title={restoreLabel}
      >
        {maximized ? <Copy aria-hidden="true" /> : <Square aria-hidden="true" />}
      </button>
      <button
        type="button"
        className="window-control window-control--close"
        onClick={close}
        aria-label="Close"
        title="Close"
      >
        <X aria-hidden="true" />
      </button>
    </div>
  );
}
