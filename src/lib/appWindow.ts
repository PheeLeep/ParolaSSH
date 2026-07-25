/** The native window this webview lives in.
 *
 *  The window is undecorated on Windows and Linux, so the navbar doubles as
 *  the titlebar: it carries `data-tauri-drag-region` and renders its own
 *  minimize / maximize / close buttons. macOS keeps native decorations with
 *  an overlay titlebar instead (see tauri.macos.conf.json), because drawing
 *  fake traffic lights convinces nobody.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

export const appWindow = getCurrentWindow();

/** On macOS the native traffic lights overlay the navbar — render no custom
 *  controls, just leave room for them on the left. */
export const isMacOS = navigator.userAgent.includes("Mac");
