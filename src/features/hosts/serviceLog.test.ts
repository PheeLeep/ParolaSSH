import { describe, expect, it } from "vitest";

import { detectLevel, parseServiceLog } from "./serviceLog";

describe("parseServiceLog", () => {
  it("splits journal lines and reads bracketed levels", () => {
    const [row] = parseServiceLog([
      "2026-10-01T14:40:06+08:00 dbbproject GLVaultAPI[1628284]: [WRN] slow request",
    ]);
    expect(row).toMatchObject({
      time: "14:40:06",
      level: "warn",
      source: "GLVaultAPI[1628284]",
      message: "[WRN] slow request",
    });
  });

  it("folds untimestamped lines into the previous row", () => {
    const rows = parseServiceLog([
      "2026-10-01T14:40:06+08:00 h app[1]: [ERR] boom",
      "   at Foo.Bar()",
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0].message).toBe("[ERR] boom\n   at Foo.Bar()");
  });

  it("keeps journal markers as standalone rows", () => {
    const rows = parseServiceLog(["2026-10-01T14:40:06+08:00 h a[1]: hi", "-- Boot abc --"]);
    expect(rows[1]).toMatchObject({ time: "", message: "-- Boot abc --" });
  });

  it("parses SCM events and flags crashes", () => {
    const rows = parseServiceLog([
      "2026-07-20T10:15:30.123  [7036]  The Print Spooler service entered the running state.",
      "2026-07-19T08:00:01.000  [7034]  The Windows Update service terminated unexpectedly.",
    ]);
    expect(rows.map((r) => r.level)).toEqual(["info", "error"]);
    expect(rows[0].time).toBe("10:15:30");
  });
});

describe("detectLevel", () => {
  it("falls back to keywords", () => {
    expect(detectLevel("connection failed")).toBe("error");
    expect(detectLevel("all good")).toBe("none");
  });
});
