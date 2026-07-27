/** The native window this webview lives in.
 *
 *  Undecorated on Windows and Linux, so the navbar doubles as the titlebar:
 *  it carries `data-tauri-drag-region` and its own window buttons. macOS keeps
 *  native decorations with an overlay titlebar (see tauri.macos.conf.json).
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

export const appWindow = getCurrentWindow();

/** On macOS the native traffic lights overlay the navbar — render no custom
 *  controls, just leave room for them on the left. */
export const isMacOS = navigator.userAgent.includes("Mac");

/** Which titlebar convention this machine actually uses. The navbar can be
 *  told to imitate another one (Settings › Navigation layout); this stays the
 *  answer for what the *window* really does. */
export const hostNavStyle: "macos" | "windows" | "linux" = isMacOS
  ? "macos"
  : navigator.userAgent.includes("Windows")
    ? "windows"
    : "linux";
