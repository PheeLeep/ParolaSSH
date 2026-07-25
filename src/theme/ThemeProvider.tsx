import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

/** What the user picked. `system` follows the OS appearance setting. */
export type ThemeMode = "light" | "dark" | "system";

/** What actually gets painted — `system` is resolved away. */
export type ResolvedTheme = "light" | "dark";

export const THEME_STORAGE_KEY = "parolassh:theme";

type ThemeContextValue = {
  mode: ThemeMode;
  resolved: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");

function readStoredMode(): ThemeMode {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === "light" || stored === "dark" || stored === "system") {
      return stored;
    }
  } catch {
    // localStorage can be unavailable (private mode, embedded webview policy)
  }
  return "system";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(readStoredMode);
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(() =>
    darkQuery().matches ? "dark" : "light",
  );

  // Keep following the OS even while the app is open.
  useEffect(() => {
    const query = darkQuery();
    const onChange = (event: MediaQueryListEvent) =>
      setSystemTheme(event.matches ? "dark" : "light");
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const resolved: ResolvedTheme = mode === "system" ? systemTheme : mode;

  // Bootstrap 5.3 reads `data-bs-theme`; `color-scheme` fixes native
  // scrollbars, form controls and the webview's default canvas colour.
  useEffect(() => {
    document.documentElement.setAttribute("data-bs-theme", resolved);
    document.documentElement.style.colorScheme = resolved;
  }, [resolved]);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // non-fatal: the theme just won't survive a restart
    }
  }, []);

  const value = useMemo(
    () => ({ mode, resolved, setMode }),
    [mode, resolved, setMode],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error("useTheme must be used inside a <ThemeProvider>");
  }
  return context;
}
