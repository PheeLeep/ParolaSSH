import { useState } from "react";
import { AppNavbar } from "./components/AppNavbar";
import { HostSidebar } from "./components/HostSidebar";
import { HostDetail } from "./features/hosts/HostDetail";
import { HostsPage } from "./features/hosts/HostsPage";
import { HostsProvider } from "./features/hosts/HostsProvider";
import { AuditPage } from "./features/keys/AuditPage";
import { KeyDetail } from "./features/keys/KeyDetail";
import { KeysPage } from "./features/keys/KeysPage";
import { KeysProvider } from "./features/keys/KeysProvider";
import { WelcomeScreen } from "./features/welcome/WelcomeScreen";
import { ThemeProvider } from "./theme/ThemeProvider";
import type { View } from "./navigation";

function App() {
  return (
    <ThemeProvider>
      <HostsProvider>
        <KeysProvider>
          <AppShell />
        </KeysProvider>
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
          {view.kind === "keys" && <KeysPage onNavigate={setView} />}
          {view.kind === "key" && (
            <KeyDetail keyId={view.keyId} onNavigate={setView} />
          )}
          {view.kind === "audit" && <AuditPage onNavigate={setView} />}
        </main>
      </div>
    </div>
  );
}

export default App;
