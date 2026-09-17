import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { formatAbsolute, formatRelative } from "./format";

const NOW = Date.parse("2026-09-17T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

describe("formatRelative", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
  });
  afterEach(() => vi.useRealTimers());

  it("names missing and unparseable timestamps", () => {
    expect(formatRelative(null)).toBe("Never");
    expect(formatRelative("not a date")).toBe("Unknown");
  });

  it("says just now under a minute", () => {
    expect(formatRelative(ago(59_000))).toBe("Just now");
  });

  it("steps through minutes, hours and days", () => {
    expect(formatRelative(ago(5 * 60_000))).toBe(relative.format(-5, "minute"));
    expect(formatRelative(ago(3 * 3_600_000))).toBe(relative.format(-3, "hour"));
    expect(formatRelative(ago(2 * 86_400_000))).toBe(relative.format(-2, "day"));
  });

  it("falls back to a date after a week", () => {
    const old = ago(8 * 86_400_000);
    expect(formatRelative(old)).toBe(formatAbsolute(old));
  });
});

describe("formatAbsolute", () => {
  it("names missing and unparseable timestamps", () => {
    expect(formatAbsolute(null)).toBe("Never connected");
    expect(formatAbsolute("nope")).toBe("Unknown");
  });
});
