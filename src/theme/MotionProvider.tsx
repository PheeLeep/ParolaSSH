import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

/** What the user picked. `system` follows the OS reduced-motion setting. */
export type MotionMode = "system" | "full" | "reduced" | "off";

/** What actually gets applied - `system` is resolved away. */
export type ResolvedMotion = "full" | "reduced" | "off";

export const MOTION_STORAGE_KEY = "parolassh:motion";

type MotionContextValue = {
  mode: MotionMode;
  resolved: ResolvedMotion;
  setMode: (mode: MotionMode) => void;
};

const MotionContext = createContext<MotionContextValue | null>(null);

const reduceQuery = () => window.matchMedia("(prefers-reduced-motion: reduce)");

function readStoredMode(): MotionMode {
  try {
    const stored = localStorage.getItem(MOTION_STORAGE_KEY);
    if (
      stored === "system" ||
      stored === "full" ||
      stored === "reduced" ||
      stored === "off"
    ) {
      return stored;
    }
  } catch {
    // localStorage can be unavailable (private mode, embedded webview policy)
  }
  return "system";
}

export function MotionProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<MotionMode>(readStoredMode);
  const [systemReduced, setSystemReduced] = useState(() => reduceQuery().matches);

  // Keep following the OS even while the app is open.
  useEffect(() => {
    const query = reduceQuery();
    const onChange = (event: MediaQueryListEvent) =>
      setSystemReduced(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const resolved: ResolvedMotion =
    mode === "system" ? (systemReduced ? "reduced" : "full") : mode;

  // All motion is expressed through duration/travel tokens keyed off this
  // attribute, so one value re-tunes every animation in the app at once.
  useEffect(() => {
    document.documentElement.setAttribute("data-motion", resolved);
  }, [resolved]);

  const setMode = useCallback((next: MotionMode) => {
    setModeState(next);
    try {
      localStorage.setItem(MOTION_STORAGE_KEY, next);
    } catch {
      // non-fatal: the preference just won't survive a restart
    }
  }, []);

  const value = useMemo(
    () => ({ mode, resolved, setMode }),
    [mode, resolved, setMode],
  );

  return (
    <MotionContext.Provider value={value}>{children}</MotionContext.Provider>
  );
}

export function useMotion(): MotionContextValue {
  const context = useContext(MotionContext);
  if (!context) {
    throw new Error("useMotion must be used inside a <MotionProvider>");
  }
  return context;
}
