import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  bindingFromChord,
  chordsFor,
  eventToChord,
  formatChord,
  hostChord,
  isTerminalHotkey,
  resetAll,
  setBinding,
  validate,
} from "./keybindings";

vi.mock("./appWindow", () => ({ isMacOS: false }));

const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

describe("keybindings", () => {
  beforeEach(() => resetAll());

  it("turns a keydown into a chord", () => {
    expect(eventToChord(key({ code: "Digit3", key: "3", ctrlKey: true, altKey: true }))).toBe("ctrl+alt+3");
    expect(eventToChord(key({ code: "ControlLeft", key: "Control", ctrlKey: true }))).toBeNull();
  });

  it("rejects reserved and clashing chords", () => {
    expect(validate("nav.keys", "f5")).toMatch(/modifier/);
    expect(validate("nav.keys", "ctrl+alt+f2")).toMatch(/TTY/);
    expect(validate("nav.keys", "ctrl+c")).toMatch(/shell/);
    expect(validate("nav.keys", "alt+1")).toMatch(/Home/);
    expect(validate("nav.keys", "alt+9")).toBeNull();
  });

  it("applies an override to hosts and the terminal filter", () => {
    expect(hostChord(2)).toBe("ctrl+alt+2");
    setBinding("hosts", [bindingFromChord("hosts", "ctrl+shift+5")]);
    expect(hostChord(2)).toBe("ctrl+shift+2");
    expect(isTerminalHotkey(key({ code: "Digit2", key: "2", ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(isTerminalHotkey(key({ code: "Digit2", key: "2", ctrlKey: true, altKey: true }))).toBe(false);
    expect(chordsFor("nav.keys")).toEqual(["alt+3"]);
  });

  it("formats chords for display", () => {
    expect(formatChord("ctrl+shift+bracketleft")).toBe("Ctrl+Shift+[");
  });
});
