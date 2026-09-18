import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { connectionInfo } from "../../../test/connection";
import type { ConnectionInfo, DefenderReport } from "../types";
import { SecurityPane } from "./SecurityPane";

let connection: ConnectionInfo;
vi.mock("../HostsProvider", () => ({ useHosts: () => ({ getConnection: () => connection }) }));
vi.mock("../ElevationProvider", () => ({ useElevation: () => vi.fn() }));
vi.mock("./AuditPane", () => ({ AuditPane: () => <p>audit</p> }));

const report = (overrides: Partial<DefenderReport> = {}): DefenderReport => ({
  verdict: "protected",
  summary: "Defender is on and up to date.",
  checks: [{ label: "Real-time protection", state: "good", value: "On" }],
  detections: [],
  recentDetections: 0,
  otherAntivirus: [],
  productVersion: "4.18.24090.11",
  note: null,
  command: "powershell ...",
  ...overrides,
});

describe("SecurityPane Defender section", () => {
  beforeEach(() => {
    connection = connectionInfo({ os: "windows" });
  });

  it("is offered only on Windows", () => {
    connection = connectionInfo({ os: "linux" });
    render(<SecurityPane hostId="h1" />);
    expect(screen.queryByRole("radio", { name: /defender/i })).not.toBeInTheDocument();
  });

  it("sits right after the audit", () => {
    render(<SecurityPane hostId="h1" />);
    const names = screen.getAllByRole("radio").map((radio) => radio.textContent?.trim());
    expect(names.indexOf("Defender")).toBe(names.indexOf("Audit") + 1);
  });

  it("shows the verdict, checks and an unremoved threat", async () => {
    mockIPC((cmd) =>
      cmd === "read_defender"
        ? report({
            verdict: "atRisk",
            summary: "Defender found a threat it has not removed.",
            detections: [{ name: "Virus:DOS/EICAR_Test_File", detected: "2026-09-17 10:00", active: true, resolved: false }],
            recentDetections: 1,
          })
        : undefined,
    );
    render(<SecurityPane hostId="h1" />);
    await userEvent.setup().click(screen.getByRole("radio", { name: /defender/i }));

    expect(await screen.findByText("At risk")).toBeInTheDocument();
    expect(screen.getByText("Real-time protection")).toBeInTheDocument();
    expect(screen.getByText("Still present")).toHaveClass("text-danger");
    expect(screen.getByText("1 in the last 30 days")).toBeInTheDocument();
  });

  it("names the third-party antivirus that took over", async () => {
    mockIPC((cmd) =>
      cmd === "read_defender"
        ? report({
            verdict: "thirdParty",
            summary: "Protected by ESET Security; Defender is in passive mode and stands aside.",
            otherAntivirus: [{ name: "ESET Security", enabled: true }],
          })
        : undefined,
    );
    render(<SecurityPane hostId="h1" />);
    await userEvent.setup().click(screen.getByRole("radio", { name: /defender/i }));

    expect(await screen.findByText("Third-party antivirus")).toBeInTheDocument();
    expect(screen.getByText(/Protected by ESET Security/)).toBeInTheDocument();
    expect(screen.getByText("No threats on record.")).toBeInTheDocument();
  });
});
