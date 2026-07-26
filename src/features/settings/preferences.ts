import * as api from "../hosts/api";

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

/** Terminal typography. `family` is a CSS font stack, `size` is in px. */
export type TerminalFont = {
  family: string;
  size: number;
};

export const TERMINAL_FONT_STORAGE_KEY = "parolassh:terminal-font";

const SYSTEM_MONO =
  'ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace';

export const DEFAULT_TERMINAL_FONT: TerminalFont = {
  family: SYSTEM_MONO,
  size: 13,
};

/**
 * Curated stacks rather than the installed-font list: a webview cannot
 * enumerate system fonts on every platform we ship to, so each option names
 * its face first and falls through to whatever mono the OS has.
 */
export const TERMINAL_FONT_FAMILIES: { label: string; value: string }[] = [
  { label: "System default", value: SYSTEM_MONO },
  { label: "JetBrains Mono", value: `"JetBrains Mono", ${SYSTEM_MONO}` },
  { label: "Fira Code", value: `"Fira Code", "Fira Mono", ${SYSTEM_MONO}` },
  { label: "Cascadia Code", value: `"Cascadia Code", "Cascadia Mono", ${SYSTEM_MONO}` },
  { label: "Source Code Pro", value: `"Source Code Pro", ${SYSTEM_MONO}` },
  { label: "IBM Plex Mono", value: `"IBM Plex Mono", ${SYSTEM_MONO}` },
  { label: "Hack", value: `Hack, ${SYSTEM_MONO}` },
  { label: "DejaVu Sans Mono", value: `"DejaVu Sans Mono", ${SYSTEM_MONO}` },
  { label: "Menlo", value: `Menlo, ${SYSTEM_MONO}` },
  { label: "Consolas", value: `Consolas, ${SYSTEM_MONO}` },
];

export const MIN_TERMINAL_FONT_SIZE = 9;
export const MAX_TERMINAL_FONT_SIZE = 24;

export function clampFontSize(size: number): number {
  if (!Number.isFinite(size)) return DEFAULT_TERMINAL_FONT.size;
  return Math.min(
    MAX_TERMINAL_FONT_SIZE,
    Math.max(MIN_TERMINAL_FONT_SIZE, Math.round(size)),
  );
}

let terminalFont: TerminalFont | null = null;
const fontListeners = new Set<(font: TerminalFont) => void>();

export function readTerminalFont(): TerminalFont {
  if (terminalFont) return terminalFont;

  terminalFont = DEFAULT_TERMINAL_FONT;
  try {
    const stored = localStorage.getItem(TERMINAL_FONT_STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored) as Partial<TerminalFont>;
      terminalFont = {
        family:
          typeof parsed.family === "string" && parsed.family.trim()
            ? parsed.family
            : DEFAULT_TERMINAL_FONT.family,
        size: clampFontSize(Number(parsed.size)),
      };
    }
  } catch {
    // Unreadable or malformed: fall back to the default rather than throwing
    // on every terminal we open.
  }
  return terminalFont;
}

export function writeTerminalFont(font: TerminalFont): void {
  terminalFont = { family: font.family, size: clampFontSize(font.size) };
  try {
    localStorage.setItem(TERMINAL_FONT_STORAGE_KEY, JSON.stringify(terminalFont));
  } catch {
    // non-fatal: the preference just won't survive a restart
  }
  for (const listener of fontListeners) listener(terminalFont);
}

/** Lets open terminals follow a change to the global default. */
export function subscribeTerminalFont(
  listener: (font: TerminalFont) => void,
): () => void {
  fontListeners.add(listener);
  return () => {
    fontListeners.delete(listener);
  };
}

/* ── Transfers ─────────────────────────────────────────────────────────── */

export const MAX_CONCURRENT_STORAGE_KEY = "parolassh:max-concurrent-transfers";

export const DEFAULT_MAX_CONCURRENT_TRANSFERS = 3;
export const MIN_MAX_CONCURRENT_TRANSFERS = 1;
export const MAX_MAX_CONCURRENT_TRANSFERS = 8;

export function clampConcurrency(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_MAX_CONCURRENT_TRANSFERS;
  return Math.min(
    MAX_MAX_CONCURRENT_TRANSFERS,
    Math.max(MIN_MAX_CONCURRENT_TRANSFERS, Math.round(value)),
  );
}

export function readMaxConcurrentTransfers(): number {
  try {
    const stored = localStorage.getItem(MAX_CONCURRENT_STORAGE_KEY);
    if (stored !== null) return clampConcurrency(Number(stored));
  } catch {
    // localStorage can be unavailable (private mode, embedded webview policy)
  }
  return DEFAULT_MAX_CONCURRENT_TRANSFERS;
}

/** Store the cap and tell Rust, which is the one that enforces it. The webview
 *  only remembers the choice; the queue lives in the backend. */
export async function writeMaxConcurrentTransfers(value: number): Promise<number> {
  const clamped = clampConcurrency(value);
  try {
    localStorage.setItem(MAX_CONCURRENT_STORAGE_KEY, String(clamped));
  } catch {
    // non-fatal: the preference just won't survive a restart
  }
  // Rust clamps too, and its answer wins — the two ranges are meant to agree,
  // and if they ever drift the enforcing side is the truthful one.
  return api.setMaxConcurrentTransfers(clamped);
}

/** Push the remembered cap to Rust at launch. The backend starts at its own
 *  default each run, so without this a saved preference would be ignored until
 *  the user opened Settings and changed it. */
export async function applyStoredConcurrency(): Promise<void> {
  try {
    await api.setMaxConcurrentTransfers(readMaxConcurrentTransfers());
  } catch {
    // The default is already in force; nothing to recover.
  }
}
