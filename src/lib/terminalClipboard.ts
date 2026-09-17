import type { Terminal } from "@xterm/xterm";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { isMacOS } from "./appWindow";

/** Ctrl+Shift+C/V (Cmd+C/V on macOS) copy and paste instead of reaching the
 *  shell as ^C/^V. Goes through the native clipboard because WebKitGTK can
 *  refuse or prompt on `navigator.clipboard.readText`. */
export function bindTerminalClipboard(terminal: Terminal): void {
  terminal.attachCustomKeyEventHandler((event) => {
    const chord = isMacOS
      ? event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
      : event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
    const key = event.key.toLowerCase();
    if (!chord || (key !== "c" && key !== "v")) return true;

    // Swallow keyup/keypress too, or the bare key leaks through afterwards.
    event.preventDefault();
    if (event.type !== "keydown") return false;

    if (key === "c") {
      const selection = terminal.getSelection();
      if (selection) void writeText(selection).catch(() => undefined);
    } else if (!terminal.options.disableStdin) {
      void readText()
        .then((text) => text && terminal.paste(text))
        .catch(() => undefined);
    }
    return false;
  });
}
