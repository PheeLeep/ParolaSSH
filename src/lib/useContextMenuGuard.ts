import { useEffect } from "react";

/** Where the native menu is still wanted: text fields need copy/paste, and
 *  xterm relies on the browser menu for its paste entry. */
function keepsNativeMenu(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return true;
  return target.closest(".xterm") !== null;
}

const DEVTOOLS_KEYS = new Set(["i", "j", "c"]);

/** Suppresses the webview's own chrome so the app reads as a desktop program.
 *  Presentation, not protection - any Tauri frontend is inspectable, and this
 *  only keeps "Inspect Element" from showing up mid-demo. */
export function useContextMenuGuard(): void {
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (keepsNativeMenu(event.target)) return;
      event.preventDefault();
    };

    // Left available in development, so debugging does not mean editing this.
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
