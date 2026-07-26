import { useEffect, useState, type ReactNode } from "react";
import { Button, Card, Form, Spinner } from "react-bootstrap";
import {
  FolderOpen,
  Home,
  Info,
  Minus,
  Monitor,
  Moon,
  Plus,
  Server,
  Sparkles,
  Sun,
  Waves,
  Zap,
} from "lucide-react";
import { Segmented } from "../../components/Segmented";
import { revealInFileManager } from "../../lib/openExternal";
import { useMotion, type MotionMode } from "../../theme/MotionProvider";
import { useTheme, type ThemeMode } from "../../theme/ThemeProvider";
import * as keysApi from "../keys/api";
import type { SshLocation } from "../keys/types";
import {
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  TERMINAL_FONT_FAMILIES,
  readStartupView,
  readTerminalFont,
  writeStartupView,
  writeTerminalFont,
  type StartupView,
  type TerminalFont,
} from "./preferences";
import type { Navigate } from "../../navigation";

export function SettingsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { mode: themeMode, setMode: setThemeMode } = useTheme();
  const { mode: motionMode, setMode: setMotionMode } = useMotion();
  const [startup, setStartup] = useState<StartupView>(readStartupView);
  const [font, setFont] = useState<TerminalFont>(readTerminalFont);
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

  const changeStartup = (next: StartupView) => {
    setStartup(next);
    writeStartupView(next);
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
            last
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
        </Card.Body>
      </Card>

      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-3">Startup</h2>

          <SettingRow title="Opening screen" hint="Where ParolaSSH lands on launch." last>
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
        </Card.Body>
      </Card>

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
