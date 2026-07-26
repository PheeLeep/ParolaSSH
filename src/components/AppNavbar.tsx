import { PanelLeft } from "lucide-react";
import { VpnIndicator } from "../features/vpn/VpnIndicator";
import { ThemeToggle } from "../theme/ThemeToggle";
import { isMacOS } from "../lib/appWindow";
import { AppIcon } from "./AppIcon";
import { WindowControls } from "./WindowControls";
import type { Navigate } from "../navigation";

/**
 * The navbar is also the titlebar: the window is undecorated, so this strip
 * carries the drag region and (outside macOS) the window controls.
 *
 * `data-tauri-drag-region` only applies to the exact element clicked, so it
 * is repeated on the inert brand elements; interactive children stay
 * clickable simply by not carrying it.
 */
export function AppNavbar({
  sidebarHidden,
  onToggleSidebar,
  onNavigate,
}: {
  sidebarHidden: boolean;
  onToggleSidebar: () => void;
  onNavigate: Navigate;
}) {
  return (
    <header
      className={`app-navbar${isMacOS ? " app-navbar--mac" : ""}`}
      data-tauri-drag-region
    >
      <button
        type="button"
        className="icon-button"
        onClick={onToggleSidebar}
        aria-pressed={!sidebarHidden}
        aria-label={sidebarHidden ? "Show sidebar" : "Hide sidebar"}
        title={sidebarHidden ? "Show sidebar" : "Hide sidebar"}
      >
        <PanelLeft aria-hidden="true" />
      </button>

      <span className="app-brand" data-tauri-drag-region>
        <span className="app-brand__mark" aria-hidden="true" data-tauri-drag-region>
          <AppIcon variant="simple" data-tauri-drag-region />
        </span>
        ParolaSSH
      </span>

      <div className="ms-auto d-flex align-items-center gap-1">
        <VpnIndicator onOpen={() => onNavigate({ kind: "vpn" })} />
        <ThemeToggle />
      </div>

      {!isMacOS && <WindowControls />}
    </header>
  );
}
