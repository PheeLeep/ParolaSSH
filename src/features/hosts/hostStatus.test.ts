import { describe, expect, it } from "vitest";

import { statusFor } from "./hostStatus";
import type { ConnectionInfo, HostHealth } from "./types";

const session = { hostId: "a" } as ConnectionInfo;
const health = (connected: boolean, reachable: boolean): HostHealth => ({
  hostId: "a",
  connected,
  reachable,
  latencyMs: null,
});

describe("statusFor", () => {
  it("is unknown before any probe", () => {
    expect(statusFor("a", {}, {})).toBe("unknown");
  });

  it("trusts a fresh session over a stale heartbeat", () => {
    expect(statusFor("a", { a: session }, { a: health(false, true) })).toBe("connected");
  });

  it("counts a heartbeat that saw the session as connected", () => {
    expect(statusFor("a", {}, { a: health(true, true) })).toBe("connected");
  });

  it("separates reachable without a session from unreachable", () => {
    expect(statusFor("a", {}, { a: health(false, true) })).toBe("reachable");
    expect(statusFor("a", {}, { a: health(false, false) })).toBe("offline");
  });
});
