import { Dropdown } from "react-bootstrap";
import { useTheme, type ThemeMode } from "./ThemeProvider";

const OPTIONS: { mode: ThemeMode; label: string; icon: string }[] = [
  { mode: "light", label: "Light", icon: "bi-sun-fill" },
  { mode: "dark", label: "Dark", icon: "bi-moon-stars-fill" },
  { mode: "system", label: "System", icon: "bi-circle-half" },
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
        <i className={`bi ${active.icon}`} aria-hidden="true" />
      </Dropdown.Toggle>

      <Dropdown.Menu>
        {OPTIONS.map((option) => (
          <Dropdown.Item
            key={option.mode}
            active={option.mode === mode}
            onClick={() => setMode(option.mode)}
            className="d-flex align-items-center gap-2"
          >
            <i className={`bi ${option.icon}`} aria-hidden="true" />
            <span className="flex-grow-1">{option.label}</span>
            {option.mode === mode && (
              <i className="bi bi-check2" aria-hidden="true" />
            )}
          </Dropdown.Item>
        ))}
      </Dropdown.Menu>
    </Dropdown>
  );
}
