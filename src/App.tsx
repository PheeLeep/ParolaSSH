import { useState } from "react";
import { AppNavbar } from "./components/AppNavbar";
import { HostSidebar } from "./components/HostSidebar";
import { HostDetail } from "./features/hosts/HostDetail";
import { HostsPage } from "./features/hosts/HostsPage";
import { HostsProvider } from "./features/hosts/HostsProvider";
import { WelcomeScreen } from "./features/welcome/WelcomeScreen";
import { ThemeProvider } from "./theme/ThemeProvider";
import type { View } from "./navigation";

function App() {
  return (
    <ThemeProvider>
      <HostsProvider>
        <AppShell />
      </HostsProvider>
    </ThemeProvider>
  );
}

function AppShell() {
  // The welcome screen is the landing pane on every launch.
  const [view, setView] = useState<View>({ kind: "welcome" });
  const [sidebarHidden, setSidebarHidden] = useState(false);

  return (
    <div className="app-shell">
      <AppNavbar
        sidebarHidden={sidebarHidden}
        onToggleSidebar={() => setSidebarHidden((hidden) => !hidden)}
      />

      <div className="app-body">
        <HostSidebar view={view} onNavigate={setView} hidden={sidebarHidden} />

        <main className="app-main">
          {view.kind === "welcome" && <WelcomeScreen onNavigate={setView} />}
          {view.kind === "hosts" && <HostsPage onNavigate={setView} />}
          {view.kind === "host" && (
            <HostDetail hostId={view.hostId} onNavigate={setView} />
          )}
        </main>
      </div>
    </div>
  );
}

export default App;
