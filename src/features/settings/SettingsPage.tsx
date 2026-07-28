import { useEffect, useState, type ReactNode } from "react";
import { Button, Card, Form, Spinner } from "react-bootstrap";
import {
  ArrowUpDown,
  ChevronDown,
  ChevronUp,
  Equal,
  FolderOpen,
  Home,
  Info,
  Minus,
  Monitor,
  Moon,
  Palette,
  Plus,
  Power,
  Server,
  ScrollText,
  Sparkles,
  SquareTerminal,
  Sun,
  Waves,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { Segmented } from "../../components/Segmented";
import { LogsPanel } from "./LogsPanel";
import { revealInFileManager } from "../../lib/openExternal";
import { useMotion, type MotionMode } from "../../theme/MotionProvider";
import { useTheme, type ThemeMode } from "../../theme/ThemeProvider";
import * as keysApi from "../keys/api";
import type { SshLocation } from "../keys/types";
import {
  MAX_MAX_CONCURRENT_TRANSFERS,
  MAX_TERMINAL_FONT_SIZE,
  MIN_MAX_CONCURRENT_TRANSFERS,
  MIN_TERMINAL_FONT_SIZE,
  TERMINAL_FONT_FAMILIES,
  clampConcurrency,
  readAutoAudit,
  readDefaultTransferPriority,
  readMaxConcurrentTransfers,
  readNavLayout,
  readStartupView,
  readTerminalFont,
  writeAutoAudit,
  writeDefaultTransferPriority,
  writeMaxConcurrentTransfers,
  writeNavLayout,
  writeStartupView,
  writeTerminalFont,
  type NavLayout,
  type StartupView,
  type TerminalFont,
} from "./preferences";
import { hostNavStyle } from "../../lib/appWindow";
import type { TransferPriority } from "../hosts/types";
import type { Navigate } from "../../navigation";

type SettingsTab =
  | "appearance"
  | "startup"
  | "transfers"
  | "terminal"
  | "files"
  | "logs";

const TABS: { id: SettingsTab; label: string; Icon: LucideIcon }[] = [
  { id: "appearance", label: "Appearance", Icon: Palette },
  { id: "startup", label: "Startup", Icon: Power },
  { id: "transfers", label: "Transfers", Icon: ArrowUpDown },
  { id: "terminal", label: "Terminal", Icon: SquareTerminal },
  { id: "files", label: "Files", Icon: FolderOpen },
  { id: "logs", label: "Logs", Icon: ScrollText },
];

const NAV_LAYOUTS: { value: NavLayout; label: string }[] = [
  { value: "auto", label: "Auto (host machine OS)" },
  { value: "macos", label: "macOS" },
  { value: "windows", label: "Windows" },
  { value: "linux", label: "Linux" },
];

const HOST_NAV_LABEL = { macos: "macOS", windows: "Windows", linux: "Linux" }[
  hostNavStyle
];

export function SettingsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { mode: themeMode, setMode: setThemeMode } = useTheme();
  const { mode: motionMode, setMode: setMotionMode } = useMotion();
  const [tab, setTab] = useState<SettingsTab>("appearance");
  const [navLayout, setNavLayout] = useState<NavLayout>(readNavLayout);
  const [startup, setStartup] = useState<StartupView>(readStartupView);
  const [autoAudit, setAutoAudit] = useState<boolean>(readAutoAudit);
  const [font, setFont] = useState<TerminalFont>(readTerminalFont);
  const [concurrency, setConcurrency] = useState(readMaxConcurrentTransfers);
  const [defaultPriority, setDefaultPriority] = useState<TransferPriority>(
    readDefaultTransferPriority,
  );
  const [sshDir, setSshDir] = useState<SshLocation | null>(null);

  useEffect(() => {
    let cancelled = false;
    keysApi
      .sshLocation()
      .then((location) => {
        if (!cancelled) setSshDir(location);
      })
      .catch(() => {
        // Non-fatal: the row just stays on its loading state.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // The navbar subscribes, so the titlebar restyles as the select changes.
  const changeNavLayout = (next: NavLayout) => {
    setNavLayout(next);
    writeNavLayout(next);
  };

  const changeAutoAudit = (next: boolean) => {
    setAutoAudit(next);
    writeAutoAudit(next);
  };

  const changeStartup = (next: StartupView) => {
    setStartup(next);
    writeStartupView(next);
  };

  // Rust enforces the cap and clamps it; its answer is what we show back, so
  // the number on screen is always the one actually in force.
  const changeConcurrency = (next: number) => {
    setConcurrency(clampConcurrency(next));
    void writeMaxConcurrentTransfers(next).then(setConcurrency);
  };

  // Read at enqueue time, so this only affects transfers queued from now on.
  const changeDefaultPriority = (next: TransferPriority) => {
    setDefaultPriority(next);
    writeDefaultTransferPriority(next);
  };

  // Open terminals follow this immediately, except where a tab set its own.
  const changeFont = (patch: Partial<TerminalFont>) => {
    const next = { ...font, ...patch };
    setFont(next);
    writeTerminalFont(next);
  };

  return (
    <div className="page">
      <header className="mb-4">
        <h1 className="page-title">Settings</h1>
        <p className="text-body-secondary mb-0">
          Preferences are stored on this machine only.
        </p>
      </header>

      <nav className="feature-nav" aria-label="Settings sections">
        {TABS.map((entry) => (
          <button
            key={entry.id}
            type="button"
            className={`feature-nav__item${tab === entry.id ? " is-active" : ""}`}
            onClick={() => setTab(entry.id)}
            aria-current={tab === entry.id ? "page" : undefined}
          >
            <entry.Icon className="icon-sm" aria-hidden="true" />
            {entry.label}
          </button>
        ))}
      </nav>

      {tab === "appearance" && (
      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-1">Appearance</h2>
          <p className="text-body-secondary small mb-3">
            Applies immediately and is remembered between launches.
          </p>

          <SettingRow
            title="Theme"
            hint="System follows your desktop's light or dark preference."
          >
            <Segmented<ThemeMode>
              label="Theme"
              value={themeMode}
              onChange={setThemeMode}
              options={[
                { value: "light", label: "Light", Icon: Sun },
                { value: "dark", label: "Dark", Icon: Moon },
                { value: "system", label: "System", Icon: Monitor },
              ]}
            />
          </SettingRow>

          <SettingRow
            title="Motion"
            hint="Reduced keeps fades but removes movement. Off disables animation entirely; progress spinners always keep turning."
          >
            <Segmented<MotionMode>
              label="Motion"
              value={motionMode}
              onChange={setMotionMode}
              options={[
                { value: "system", label: "System", Icon: Monitor },
                { value: "full", label: "Full", Icon: Sparkles },
                { value: "reduced", label: "Reduced", Icon: Waves },
                { value: "off", label: "Off", Icon: Zap },
              ]}
            />
          </SettingRow>

          <SettingRow
            title="Navigation layout"
            hint={`Which platform's title bar the top navigation imitates. Auto follows this machine - ${HOST_NAV_LABEL}. The window's own decorations don't change.`}
            last
          >
            <Form.Select
              value={navLayout}
              onChange={(event) => changeNavLayout(event.target.value as NavLayout)}
              aria-label="Navigation layout"
              style={{ maxWidth: "18rem" }}
            >
              {NAV_LAYOUTS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Form.Select>
          </SettingRow>
        </Card.Body>
      </Card>
      )}

      {tab === "startup" && (
      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-3">Startup</h2>

          <SettingRow title="Opening screen" hint="Where ParolaSSH lands on launch.">
            <Segmented<StartupView>
              label="Opening screen"
              value={startup}
              onChange={changeStartup}
              options={[
                { value: "welcome", label: "Home", Icon: Home },
                { value: "hosts", label: "All hosts", Icon: Server },
              ]}
            />
          </SettingRow>

          <SettingRow
            title="Check posture on connect"
            hint="Runs the Audit tab's read-only checks once a host connects, without asking. Only the unprivileged half: the checks needing root stay behind their prompt, and the report names what it had to skip."
            last
          >
            <Form.Check
              type="switch"
              id="auto-audit"
              aria-label="Check posture on connect"
              checked={autoAudit}
              onChange={(event) => changeAutoAudit(event.target.checked)}
            />
          </SettingRow>
        </Card.Body>
      </Card>
      )}

      {tab === "transfers" && (
      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-3">Transfers</h2>

          <SettingRow
            title="Transfers at once"
            hint="How many uploads and downloads run in parallel across every host. The rest wait in the queue, highest priority first. Lowering this never interrupts a transfer already running."
          >
            <div className="d-flex align-items-center gap-2">
              <Button
                variant="outline-secondary"
                size="sm"
                aria-label="Fewer at once"
                disabled={concurrency <= MIN_MAX_CONCURRENT_TRANSFERS}
                onClick={() => changeConcurrency(concurrency - 1)}
              >
                <Minus className="icon-sm" aria-hidden="true" />
              </Button>
              <span className="font-monospace" style={{ minWidth: "2ch", textAlign: "center" }}>
                {concurrency}
              </span>
              <Button
                variant="outline-secondary"
                size="sm"
                aria-label="More at once"
                disabled={concurrency >= MAX_MAX_CONCURRENT_TRANSFERS}
                onClick={() => changeConcurrency(concurrency + 1)}
              >
                <Plus className="icon-sm" aria-hidden="true" />
              </Button>
            </div>
          </SettingRow>

          <SettingRow
            title="Default priority"
            hint="What new uploads and downloads are queued at. A transfer that is still waiting can be moved up or down from its row on the Transfers page."
            last
          >
            <Segmented<TransferPriority>
              label="Default priority"
              value={defaultPriority}
              onChange={changeDefaultPriority}
              options={[
                { value: "low", label: "Low", Icon: ChevronDown },
                { value: "normal", label: "Normal", Icon: Equal },
                { value: "high", label: "High", Icon: ChevronUp },
              ]}
            />
          </SettingRow>
        </Card.Body>
      </Card>
      )}

      {tab === "terminal" && (
      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-1">Terminal</h2>
          <p className="text-body-secondary small mb-3">
            The default for every terminal. A single tab can be changed from
            its own font button without touching this.
          </p>

          <SettingRow
            title="Font family"
            hint="Falls back to the system monospace if the face isn't installed."
          >
            <Form.Select
              value={font.family}
              onChange={(event) => changeFont({ family: event.target.value })}
              aria-label="Terminal font family"
              style={{ maxWidth: "14rem" }}
            >
              {TERMINAL_FONT_FAMILIES.every(
                (option) => option.value !== font.family,
              ) && <option value={font.family}>Custom</option>}
              {TERMINAL_FONT_FAMILIES.map((option) => (
                <option key={option.label} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Form.Select>
          </SettingRow>

          <SettingRow
            title="Font size"
            hint={`Between ${MIN_TERMINAL_FONT_SIZE} and ${MAX_TERMINAL_FONT_SIZE} pixels.`}
            last
          >
            <div className="d-flex align-items-center gap-2">
              <Button
                variant="outline-secondary"
                size="sm"
                disabled={font.size <= MIN_TERMINAL_FONT_SIZE}
                onClick={() => changeFont({ size: font.size - 1 })}
                aria-label="Smaller"
              >
                <Minus aria-hidden="true" />
              </Button>
              <span className="font-monospace" style={{ minWidth: "3rem" }}>
                {font.size}px
              </span>
              <Button
                variant="outline-secondary"
                size="sm"
                disabled={font.size >= MAX_TERMINAL_FONT_SIZE}
                onClick={() => changeFont({ size: font.size + 1 })}
                aria-label="Larger"
              >
                <Plus aria-hidden="true" />
              </Button>
            </div>
          </SettingRow>
        </Card.Body>
      </Card>
      )}

      {tab === "files" && (
      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-3">Files</h2>

          <SettingRow
            title="SSH directory"
            hint={
              sshDir ? (
                <code className="user-select-auto">{sshDir.path}</code>
              ) : (
                "Locating…"
              )
            }
            last
          >
            {sshDir ? (
              <Button
                variant="outline-secondary"
                size="sm"
                disabled={!sshDir.exists}
                onClick={() => revealInFileManager(sshDir.path)}
                title={sshDir.exists ? undefined : "This directory does not exist yet"}
              >
                <FolderOpen aria-hidden="true" />
                Reveal
              </Button>
            ) : (
              <Spinner animation="border" size="sm" />
            )}
          </SettingRow>
        </Card.Body>
      </Card>
      )}

      {tab === "logs" && <LogsPanel />}

      <Button
        variant="outline-secondary"
        onClick={() => onNavigate({ kind: "about" })}
      >
        <Info aria-hidden="true" />
        About ParolaSSH
      </Button>
    </div>
  );
}

function SettingRow({
  title,
  hint,
  children,
  last = false,
}: {
  title: string;
  hint: ReactNode;
  children: ReactNode;
  last?: boolean;
}) {
  return (
    <div className={`settings-row${last ? " is-last" : ""}`}>
      <div className="settings-row__text">
        <div className="settings-row__title">{title}</div>
        <div className="settings-row__hint">{hint}</div>
      </div>
      <div className="settings-row__control">{children}</div>
    </div>
  );
}
