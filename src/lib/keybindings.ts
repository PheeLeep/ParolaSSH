import { useSyncExternalStore } from "react";
import type { View } from "../navigation";
import { isMacOS } from "./appWindow";

const STORAGE_KEY = "parolassh.keybindings";
const NAV_MOD = isMacOS ? "meta" : "alt";
const HOST_SLOTS = 9;

export type Section = "Navigation" | "Hosts" | "Terminal tabs";

export type Action = {
  id: string;
  section: Section;
  label: string;
  /** Chords in react-hotkeys-hook syntax (keys match `event.code`). */
  defaults: string[];
};

export const NAV_ACTIONS: (Action & { view: View["kind"] })[] = (
  [
    ["welcome", "Home"],
    ["hosts", "All hosts"],
    ["keys", "Keys"],
    ["vpn", "VPN"],
    ["sessions", "Sessions"],
    ["transfers", "Transfers"],
    ["settings", "Settings"],
    ["about", "About"],
  ] as const
).map(([view, label], index) => ({
  id: `nav.${view}`,
  section: "Navigation",
  label,
  defaults: [`${NAV_MOD}+${index + 1}`],
  view,
}));

export const HOSTS_ACTION = "hosts";

/** Ctrl+B is tmux's prefix, so the sidebar toggle keeps off it. */
export const ACTIONS: Action[] = [
  ...NAV_ACTIONS,
  { id: "sidebar", section: "Navigation", label: "Toggle sidebar", defaults: [isMacOS ? "meta+b" : "ctrl+shift+b"] },
  // Modifiers only: the digit 1..9 is appended per slot.
  { id: HOSTS_ACTION, section: "Hosts", label: "Open host 1 to 9 (sidebar order)", defaults: ["ctrl+alt"] },
  { id: "term.new", section: "Terminal tabs", label: "New tab", defaults: [isMacOS ? "meta+t" : "ctrl+shift+t"] },
  { id: "term.close", section: "Terminal tabs", label: "Close tab", defaults: [isMacOS ? "meta+w" : "ctrl+shift+w"] },
  {
    id: "term.prev",
    section: "Terminal tabs",
    label: "Previous tab",
    defaults: isMacOS ? ["meta+shift+bracketleft", "ctrl+pageup"] : ["ctrl+pageup", "ctrl+shift+tab"],
  },
  {
    id: "term.next",
    section: "Terminal tabs",
    label: "Next tab",
    defaults: isMacOS ? ["meta+shift+bracketright", "ctrl+pagedown"] : ["ctrl+pagedown", "ctrl+tab"],
  },
];

/** Not rebindable: the terminal or the webview owns them. */
export const FIXED_BINDINGS: { label: string; chords: string[]; section: string }[] = [
  { section: "Terminal tabs", label: "Rename the focused tab", chords: ["f2"] },
  { section: "Terminal clipboard", label: "Copy selection", chords: [isMacOS ? "meta+c" : "ctrl+shift+c"] },
  { section: "Terminal clipboard", label: "Paste", chords: [isMacOS ? "meta+v" : "ctrl+shift+v"] },
];

let overrides: Record<string, string[]> = load();
let version = 0;
const listeners = new Set<() => void>();

function load(): Record<string, string[]> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    if (!parsed || typeof parsed !== "object") return {};
    const known = new Set(ACTIONS.map((action) => action.id));
    return Object.fromEntries(
      Object.entries(parsed).filter(
        ([id, chords]) =>
          known.has(id) &&
          Array.isArray(chords) &&
          chords.length > 0 &&
          chords.every((chord) => typeof chord === "string"),
      ),
    );
  } catch {
    return {};
  }
}

function commit() {
  version++;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
  } catch {
    // Unavailable storage just means the change lasts until restart.
  }
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const getVersion = () => version;

/** Call in any component that shows or registers a chord. */
export function useKeybindings(): void {
  useSyncExternalStore(subscribe, getVersion);
}

export function chordsFor(id: string): string[] {
  return overrides[id] ?? ACTIONS.find((action) => action.id === id)?.defaults ?? [];
}

export const hostChord = (slot: number) => `${chordsFor(HOSTS_ACTION)[0]}+${slot}`;

export const isOverridden = (id: string) => id in overrides;

export function setBinding(id: string, chords: string[]) {
  overrides = { ...overrides, [id]: chords };
  commit();
}

export function resetBinding(id: string) {
  const { [id]: _removed, ...rest } = overrides;
  overrides = rest;
  commit();
}

export function resetAll() {
  overrides = {};
  commit();
}

/** Every concrete chord an action answers to. */
function expand(id: string, chords: string[]): string[] {
  return id === HOSTS_ACTION
    ? chords.flatMap((mod) => Array.from({ length: HOST_SLOTS }, (_, i) => `${mod}+${i + 1}`))
    : chords;
}

function allConcrete(): { label: string; chord: string; id: string | null }[] {
  return [
    ...ACTIONS.flatMap((action) =>
      expand(action.id, chordsFor(action.id)).map((chord) => ({ label: action.label, chord, id: action.id })),
    ),
    ...FIXED_BINDINGS.flatMap((fixed) =>
      fixed.chords.map((chord) => ({ label: fixed.label, chord, id: null })),
    ),
  ];
}

const MODIFIER_CODE = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

function keyName(event: KeyboardEvent): string {
  return event.key === "Tab" ? "tab" : event.code.toLowerCase().replace(/^(key|digit)/, "");
}

/** The chord a keydown represents, or null for a bare modifier press. */
export function eventToChord(event: KeyboardEvent): string | null {
  if (MODIFIER_CODE.test(event.code)) return null;
  const mods = [
    event.ctrlKey && "ctrl",
    event.altKey && "alt",
    event.shiftKey && "shift",
    event.metaKey && "meta",
  ].filter(Boolean);
  return [...mods, keyName(event)].join("+");
}

function matches(chord: string, event: KeyboardEvent): boolean {
  const parts = chord.split("+");
  const key = parts.pop()!;
  const has = (mod: string) => parts.includes(mod);
  return (
    keyName(event) === key &&
    has("ctrl") === event.ctrlKey &&
    has("shift") === event.shiftKey &&
    has("alt") === event.altKey &&
    has("meta") === event.metaKey
  );
}

/** True when xterm must not send the event to the shell. */
export function isTerminalHotkey(event: KeyboardEvent): boolean {
  return ACTIONS.some((action) =>
    expand(action.id, chordsFor(action.id)).some((chord) => matches(chord, event)),
  );
}

function reservedReason(chord: string): string | null {
  const parts = chord.split("+");
  const key = parts.pop()!;
  const mods = new Set(parts);
  if (mods.size === 0) return "Add at least one modifier (Ctrl, Alt, Shift or Cmd).";
  if (/^f\d+$/.test(key) && mods.has("ctrl") && mods.has("alt")) {
    return "Ctrl+Alt+F-keys switch the Linux TTY.";
  }
  const letter = /^[a-z]$/.test(key);
  if (letter && mods.size === 1 && (mods.has("ctrl") || mods.has("alt"))) {
    return "Reserved for the shell (Ctrl+letter, Alt+letter).";
  }
  if (key === "f12" || (mods.has("ctrl") && !mods.has("alt") && key === "u")) return "Reserved by the app.";
  if (mods.has("ctrl") && mods.has("shift") && ["i", "j", "c", "v"].includes(key)) return "Reserved by the app.";
  return null;
}

/** Why `chord` can't be bound to `id`, or null when it can. For the hosts
 *  action only the modifiers of the captured chord are kept. */
export function validate(id: string, chord: string): string | null {
  const candidate = id === HOSTS_ACTION ? chord.split("+").slice(0, -1).join("+") : chord;
  const probe = id === HOSTS_ACTION ? `${candidate}+1` : candidate;
  const reserved = reservedReason(probe);
  if (reserved) return reserved;
  const wanted = new Set(expand(id, [candidate]));
  const clash = allConcrete().find((entry) => entry.id !== id && wanted.has(entry.chord));
  return clash ? `Already used by "${clash.label}".` : null;
}

/** The binding to store for a captured chord. */
export const bindingFromChord = (id: string, chord: string) =>
  id === HOSTS_ACTION ? chord.split("+").slice(0, -1).join("+") : chord;

const KEY_NAMES: Record<string, string> = {
  meta: "Cmd",
  pageup: "PageUp",
  pagedown: "PageDown",
  bracketleft: "[",
  bracketright: "]",
  arrowleft: "Left",
  arrowright: "Right",
  arrowup: "Up",
  arrowdown: "Down",
};

/** "ctrl+shift+t" -> "Ctrl+Shift+T". */
export function formatChord(chord: string): string {
  return chord
    .split("+")
    .map((part) => KEY_NAMES[part] ?? part[0].toUpperCase() + part.slice(1))
    .join("+");
}

const HINTS_KEY = "parolassh.keyHints";

function loadHints(): boolean {
  try {
    return localStorage.getItem(HINTS_KEY) !== "off";
  } catch {
    return true;
  }
}

let hintsOn = loadHints();

export const hintsEnabled = () => hintsOn;

export function setHintsEnabled(on: boolean) {
  hintsOn = on;
  try {
    localStorage.setItem(HINTS_KEY, on ? "on" : "off");
  } catch {
    // Lasts until restart.
  }
  version++;
  for (const listener of listeners) listener();
}

/** Display text for an action's first chord, or null when hints are off or
 *  nothing is bound. A slot picks one of the host shortcuts. */
export function hintText(id: string, slot?: number): string | null {
  if (!hintsOn) return null;
  const chord = slot === undefined ? chordsFor(id)[0] : hostChord(slot);
  return chord ? formatChord(chord) : null;
}

/** Tooltip suffix like " (Alt+1)" for icon-only buttons. */
export const hint = (id: string): string => {
  const text = hintText(id);
  return text ? ` (${text})` : "";
};

export { HOST_SLOTS };
