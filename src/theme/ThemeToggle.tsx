import { Dropdown } from "react-bootstrap";
import {
  Check,
  Gauge,
  Monitor,
  Moon,
  Sparkles,
  Sun,
  Waves,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useTheme, type ThemeMode } from "./ThemeProvider";
import { useMotion, type MotionMode } from "./MotionProvider";

const THEMES: { mode: ThemeMode; label: string; Icon: LucideIcon }[] = [
  { mode: "light", label: "Light", Icon: Sun },
  { mode: "dark", label: "Dark", Icon: Moon },
  { mode: "system", label: "System", Icon: Monitor },
];

const MOTIONS: {
  mode: MotionMode;
  label: string;
  hint: string;
  Icon: LucideIcon;
}[] = [
  { mode: "system", label: "System", hint: "Follow OS setting", Icon: Monitor },
  { mode: "full", label: "Full", hint: "All animations", Icon: Sparkles },
  { mode: "reduced", label: "Reduced", hint: "Fades only", Icon: Waves },
  { mode: "off", label: "Off", hint: "No animation", Icon: Zap },
];

export function ThemeToggle() {
  const { mode: themeMode, setMode: setThemeMode } = useTheme();
  const { mode: motionMode, setMode: setMotionMode } = useMotion();

  const activeTheme = THEMES.find((t) => t.mode === themeMode) ?? THEMES[2];

  return (
    <Dropdown align="end">
      <Dropdown.Toggle
        variant="outline-secondary"
        size="sm"
        id="appearance-menu"
        aria-label={`Appearance: ${activeTheme.label} theme`}
      >
        <activeTheme.Icon aria-hidden="true" />
      </Dropdown.Toggle>

      <Dropdown.Menu>
        <Dropdown.Header className="d-flex align-items-center gap-2">
          <Monitor className="icon-sm" aria-hidden="true" />
          Theme
        </Dropdown.Header>
        {THEMES.map((option) => (
          <Dropdown.Item
            key={option.mode}
            active={option.mode === themeMode}
            onClick={() => setThemeMode(option.mode)}
          >
            <option.Icon aria-hidden="true" />
            <span className="flex-grow-1">{option.label}</span>
            {option.mode === themeMode && <Check aria-hidden="true" />}
          </Dropdown.Item>
        ))}

        <Dropdown.Divider />

        <Dropdown.Header className="d-flex align-items-center gap-2">
          <Gauge className="icon-sm" aria-hidden="true" />
          Motion
        </Dropdown.Header>
        {MOTIONS.map((option) => (
          <Dropdown.Item
            key={option.mode}
            active={option.mode === motionMode}
            onClick={() => setMotionMode(option.mode)}
          >
            <option.Icon aria-hidden="true" />
            <span className="flex-grow-1">
              {option.label}
              <span className="dropdown-item__hint">{option.hint}</span>
            </span>
            {option.mode === motionMode && <Check aria-hidden="true" />}
          </Dropdown.Item>
        ))}
      </Dropdown.Menu>
    </Dropdown>
  );
}
