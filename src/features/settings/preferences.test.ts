import { beforeEach, describe, expect, it, vi } from "vitest";

/** Fresh module per test: the font and nav layout are cached after first read. */
async function load() {
  vi.resetModules();
  return import("./preferences");
}

beforeEach(() => localStorage.clear());

describe("startup view", () => {
  it("defaults to welcome and ignores unknown values", async () => {
    const prefs = await load();
    expect(prefs.readStartupView()).toBe("welcome");
    localStorage.setItem(prefs.STARTUP_STORAGE_KEY, "sessions");
    expect(prefs.readStartupView()).toBe("welcome");
  });

  it("round-trips a valid choice", async () => {
    const prefs = await load();
    prefs.writeStartupView("hosts");
    expect(prefs.readStartupView()).toBe("hosts");
  });

  it("survives storage that throws", async () => {
    const prefs = await load();
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(() => prefs.writeStartupView("hosts")).not.toThrow();
    expect(prefs.readStartupView()).toBe("welcome");
  });
});

describe("terminal font", () => {
  it("clamps size and rounds", async () => {
    const { clampFontSize, MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE, DEFAULT_TERMINAL_FONT } =
      await load();
    expect(clampFontSize(2)).toBe(MIN_TERMINAL_FONT_SIZE);
    expect(clampFontSize(99)).toBe(MAX_TERMINAL_FONT_SIZE);
    expect(clampFontSize(13.6)).toBe(14);
    expect(clampFontSize(Number.NaN)).toBe(DEFAULT_TERMINAL_FONT.size);
  });

  it("repairs a malformed stored value field by field", async () => {
    localStorage.setItem("parolassh:terminal-font", JSON.stringify({ family: "  ", size: 100 }));
    const prefs = await load();
    expect(prefs.readTerminalFont()).toEqual({
      family: prefs.DEFAULT_TERMINAL_FONT.family,
      size: prefs.MAX_TERMINAL_FONT_SIZE,
    });
  });

  it("falls back to the default on unparseable JSON", async () => {
    localStorage.setItem("parolassh:terminal-font", "{oops");
    const prefs = await load();
    expect(prefs.readTerminalFont()).toEqual(prefs.DEFAULT_TERMINAL_FONT);
  });

  it("clamps on write and notifies subscribers", async () => {
    const prefs = await load();
    const listener = vi.fn();
    const unsubscribe = prefs.subscribeTerminalFont(listener);
    prefs.writeTerminalFont({ family: "Hack", size: 1 });
    expect(listener).toHaveBeenCalledWith({ family: "Hack", size: prefs.MIN_TERMINAL_FONT_SIZE });

    unsubscribe();
    prefs.writeTerminalFont({ family: "Hack", size: 12 });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(prefs.readTerminalFont().size).toBe(12);
  });
});

describe("nav layout", () => {
  it("defaults to auto and rejects unknown styles", async () => {
    localStorage.setItem("parolassh:nav-layout", "beos");
    const prefs = await load();
    expect(prefs.readNavLayout()).toBe("auto");
  });

  it("notifies subscribers on write", async () => {
    const prefs = await load();
    const listener = vi.fn();
    prefs.subscribeNavLayout(listener);
    prefs.writeNavLayout("macos");
    expect(listener).toHaveBeenCalledWith("macos");
    expect(localStorage.getItem(prefs.NAV_LAYOUT_STORAGE_KEY)).toBe("macos");
  });
});

describe("transfers", () => {
  it("clamps the concurrency cap", async () => {
    const prefs = await load();
    expect(prefs.clampConcurrency(0)).toBe(prefs.MIN_MAX_CONCURRENT_TRANSFERS);
    expect(prefs.clampConcurrency(50)).toBe(prefs.MAX_MAX_CONCURRENT_TRANSFERS);
    expect(prefs.clampConcurrency(Number.NaN)).toBe(prefs.DEFAULT_MAX_CONCURRENT_TRANSFERS);
  });

  it("reads a stored cap through the clamp", async () => {
    localStorage.setItem("parolassh:max-concurrent-transfers", "20");
    const prefs = await load();
    expect(prefs.readMaxConcurrentTransfers()).toBe(prefs.MAX_MAX_CONCURRENT_TRANSFERS);
  });

  it("stores the clamped cap and returns what Rust enforced", async () => {
    const { mockIPC } = await import("@tauri-apps/api/mocks");
    const sent: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "set_max_concurrent_transfers") {
        sent.push(args);
        return 5;
      }
    });
    const prefs = await load();
    await expect(prefs.writeMaxConcurrentTransfers(12)).resolves.toBe(5);
    expect(localStorage.getItem(prefs.MAX_CONCURRENT_STORAGE_KEY)).toBe("8");
    expect(sent).toHaveLength(1);
  });

  it("only accepts known default priorities", async () => {
    localStorage.setItem("parolassh:default-transfer-priority", "urgent");
    const prefs = await load();
    expect(prefs.readDefaultTransferPriority()).toBe("normal");
    prefs.writeDefaultTransferPriority("high");
    expect(prefs.readDefaultTransferPriority()).toBe("high");
  });
});

describe("auto audit", () => {
  it("is off unless explicitly turned on", async () => {
    const prefs = await load();
    expect(prefs.readAutoAudit()).toBe(false);
    localStorage.setItem(prefs.AUTO_AUDIT_STORAGE_KEY, "true");
    expect(prefs.readAutoAudit()).toBe(false);
    prefs.writeAutoAudit(true);
    expect(prefs.readAutoAudit()).toBe(true);
  });
});

describe("connect details", () => {
  it("is off unless turned on", async () => {
    const prefs = await load();
    expect(prefs.readConnectDetails()).toBe(false);
    prefs.writeConnectDetails(true);
    expect(prefs.readConnectDetails()).toBe(true);
    prefs.writeConnectDetails(false);
    expect(prefs.readConnectDetails()).toBe(false);
  });
});
