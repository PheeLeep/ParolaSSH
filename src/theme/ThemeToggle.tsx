import { Dropdown } from "react-bootstrap";
import { Check, Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { useTheme, type ThemeMode } from "./ThemeProvider";

const OPTIONS: { mode: ThemeMode; label: string; Icon: LucideIcon }[] = [
  { mode: "light", label: "Light", Icon: Sun },
  { mode: "dark", label: "Dark", Icon: Moon },
  { mode: "system", label: "System", Icon: Monitor },
];

export function ThemeToggle() {
  const { mode, setMode } = useTheme();
  const active = OPTIONS.find((option) => option.mode === mode) ?? OPTIONS[2];

  return (
    <Dropdown align="end">
      <Dropdown.Toggle
        variant="outline-secondary"
        size="sm"
        id="theme-toggle"
        aria-label={`Theme: ${active.label}`}
      >
        <active.Icon aria-hidden="true" />
      </Dropdown.Toggle>

      <Dropdown.Menu>
        {OPTIONS.map((option) => (
          <Dropdown.Item
            key={option.mode}
            active={option.mode === mode}
            onClick={() => setMode(option.mode)}
          >
            <option.Icon aria-hidden="true" />
            <span className="flex-grow-1">{option.label}</span>
            {option.mode === mode && <Check aria-hidden="true" />}
          </Dropdown.Item>
        ))}
      </Dropdown.Menu>
    </Dropdown>
  );
}
