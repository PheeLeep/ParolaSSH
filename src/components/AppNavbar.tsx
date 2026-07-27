import { PanelLeft } from "lucide-react";
import { VpnIndicator } from "../features/vpn/VpnIndicator";
import { ThemeToggle } from "../theme/ThemeToggle";
import { isMacOS } from "../lib/appWindow";
import { useNavStyle } from "../features/settings/useNavStyle";
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
 *
 * Which convention it draws comes from Settings › Navigation layout, which
 * defaults to the host machine's. Two things stay tied to the real host and
 * not the preference: only a genuine macOS window has native traffic lights
 * (so only there do we draw none), and only a genuine macOS window needs the
 * left inset to clear them.
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
  const navStyle = useNavStyle();
  const nativeLights = isMacOS && navStyle === "macos";

  return (
    <header
      className={`app-navbar app-navbar--${navStyle}${
        isMacOS ? " app-navbar--mac-inset" : ""
      }`}
      data-tauri-drag-region
    >
      {navStyle === "macos" && !nativeLights && <WindowControls style="macos" />}

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

      {navStyle !== "macos" && <WindowControls style={navStyle} />}
    </header>
  );
}
