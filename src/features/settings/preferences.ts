/** Which pane opens on launch. */
export type StartupView = "welcome" | "hosts";

export const STARTUP_STORAGE_KEY = "parolassh:startup";

export function readStartupView(): StartupView {
  try {
    const stored = localStorage.getItem(STARTUP_STORAGE_KEY);
    if (stored === "welcome" || stored === "hosts") return stored;
  } catch {
    // localStorage can be unavailable (private mode, embedded webview policy)
  }
  return "welcome";
}

export function writeStartupView(view: StartupView): void {
  try {
    localStorage.setItem(STARTUP_STORAGE_KEY, view);
  } catch {
    // non-fatal: the preference just won't survive a restart
  }
}
