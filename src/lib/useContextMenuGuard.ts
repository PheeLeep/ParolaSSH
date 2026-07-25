import { useEffect } from "react";

/**
 * Where the native menu is still wanted: text fields need copy/paste, and
 * xterm relies on the browser menu for its paste entry (it parks a hidden
 * textarea under the pointer on right-click precisely so that works).
 */
function keepsNativeMenu(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return true;
  return target.closest(".xterm") !== null;
}

const DEVTOOLS_KEYS = new Set(["i", "j", "c"]);

/**
 * Suppresses the webview's own chrome so the app reads as a desktop program
 * rather than a web page.
 *
 * Worth being clear about what this is: presentation, not protection. The
 * frontend of any Tauri app is inspectable by someone who wants to look, and
 * release builds already ship without devtools unless the feature is turned
 * on. This just stops "Inspect Element" showing up mid-demo.
 */
export function useContextMenuGuard(): void {
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (keepsNativeMenu(event.target)) return;
      event.preventDefault();
    };

    // Left available during development — otherwise debugging this very app
    // would mean commenting the guard out.
    const onKeyDown = (event: KeyboardEvent) => {
      if (import.meta.env.DEV) return;

      const key = event.key.toLowerCase();
      const opensDevtools =
        event.key === "F12" ||
        (event.ctrlKey && event.shiftKey && DEVTOOLS_KEYS.has(key)) ||
        (event.metaKey && event.altKey && DEVTOOLS_KEYS.has(key)) ||
        (event.ctrlKey && key === "u");

      if (opensDevtools) event.preventDefault();
    };

    document.addEventListener("contextmenu", onContextMenu);
    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("contextmenu", onContextMenu);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, []);
}
