import { PanelLeft, Settings, SquareTerminal } from "lucide-react";
import { ThemeToggle } from "../theme/ThemeToggle";

export function AppNavbar({
  sidebarHidden,
  onToggleSidebar,
}: {
  sidebarHidden: boolean;
  onToggleSidebar: () => void;
}) {
  return (
    <header className="app-navbar">
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

      <span className="app-brand">
        <span className="app-brand__mark" aria-hidden="true">
          <SquareTerminal className="icon-lg" />
        </span>
        ParolaSSH
      </span>

      <div className="ms-auto d-flex align-items-center gap-1">
        <button type="button" className="icon-button" disabled title="Settings">
          <Settings aria-hidden="true" />
          <span className="visually-hidden">Settings</span>
        </button>
        <ThemeToggle />
      </div>
    </header>
  );
}
