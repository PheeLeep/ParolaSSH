import { useState } from "react";
import { AppNavbar } from "./components/AppNavbar";
import { CloseGuard } from "./components/CloseGuard";
import { HostSidebar } from "./components/HostSidebar";
import { ElevationProvider } from "./features/hosts/ElevationProvider";
import { HostDetail } from "./features/hosts/HostDetail";
import { HostsPage } from "./features/hosts/HostsPage";
import { HostsProvider } from "./features/hosts/HostsProvider";
import { AuditPage } from "./features/keys/AuditPage";
import { KeyDetail } from "./features/keys/KeyDetail";
import { KeysPage } from "./features/keys/KeysPage";
import { KeysProvider } from "./features/keys/KeysProvider";
import { AboutPage } from "./features/settings/AboutPage";
import { readStartupView } from "./features/settings/preferences";
import { SettingsPage } from "./features/settings/SettingsPage";
import { VpnPage } from "./features/vpn/VpnPage";
import { VpnProvider } from "./features/vpn/VpnProvider";
import { WelcomeScreen } from "./features/welcome/WelcomeScreen";
import { useContextMenuGuard } from "./lib/useContextMenuGuard";
import { MotionProvider } from "./theme/MotionProvider";
import { ThemeProvider } from "./theme/ThemeProvider";
import type { View } from "./navigation";

function App() {
  return (
    <ThemeProvider>
      <MotionProvider>
        <HostsProvider>
          {/* Both need the host list, so they sit inside HostsProvider. */}
          <ElevationProvider>
            <VpnProvider>
              <KeysProvider>
                <AppShell />
              </KeysProvider>
            </VpnProvider>
          </ElevationProvider>
        </HostsProvider>
      </MotionProvider>
    </ThemeProvider>
  );
}

/** Remounts the pane on navigation so its enter animation replays. */
function paneKey(view: View): string {
  if (view.kind === "host") return `host:${view.hostId}`;
  if (view.kind === "key") return `key:${view.keyId}`;
  return view.kind;
}

function AppShell() {
  // Read once: changing the preference later should not yank the current pane.
  const [view, setView] = useState<View>(() => ({ kind: readStartupView() }));

  useContextMenuGuard();
  const [sidebarHidden, setSidebarHidden] = useState(false);

  return (
    <div className="app-shell">
      <CloseGuard />
      <AppNavbar
        sidebarHidden={sidebarHidden}
        onToggleSidebar={() => setSidebarHidden((hidden) => !hidden)}
        onNavigate={setView}
      />

      <div className="app-body">
        <HostSidebar view={view} onNavigate={setView} hidden={sidebarHidden} />

        <main className="app-main">
          <div className="app-pane" key={paneKey(view)}>
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
            {view.kind === "vpn" && <VpnPage onNavigate={setView} />}
            {view.kind === "settings" && <SettingsPage onNavigate={setView} />}
            {view.kind === "about" && <AboutPage onNavigate={setView} />}
          </div>
        </main>
      </div>
    </div>
  );
}

export default App;
