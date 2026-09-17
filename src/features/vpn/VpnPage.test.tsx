import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { hostRow } from "../../test/fixtures";
import type { HostRow } from "../hosts/HostsProvider";
import type { VpnBinding, VpnResource, VpnStatus } from "./types";
import { VpnPage } from "./VpnPage";

type Vpn = {
  statuses: VpnStatus[];
  resources: VpnResource[];
  resourcesSeenAt: string | null;
  bindings: Record<string, VpnBinding>;
};

let vpn: Vpn;
let hosts: HostRow[];
const refresh = vi.fn(async () => {});

vi.mock("./VpnProvider", () => ({
  useVpn: () => ({
    statuses: vpn.statuses,
    resources: vpn.resources,
    resourcesSeenAt: vpn.resourcesSeenAt,
    bindingFor: (hostname: string) => vpn.bindings[hostname],
    lastChecked: null,
    refresh,
  }),
}));
vi.mock("../hosts/HostsProvider", () => ({ useHosts: () => ({ hosts }) }));
vi.mock("./TailscaleImportDialog", () => ({ TailscaleImportDialog: () => null }));

const status = (kind: VpnStatus["kind"], up: boolean, detail: string): VpnStatus => ({
  kind,
  installed: true,
  up,
  detail,
});

const resource = (overrides: Partial<VpnResource> = {}): VpnResource => ({
  name: "acme lab",
  address: "192.168.9.0/24",
  alias: null,
  authStatus: "Auth expires in 4 days",
  needsAuth: false,
  ...overrides,
});

beforeEach(() => {
  hosts = [];
  vpn = { statuses: [], resources: [], resourcesSeenAt: null, bindings: {} };
});

async function openTab(name: string) {
  const user = userEvent.setup();
  render(<VpnPage onNavigate={vi.fn()} />);
  await user.click(
    within(screen.getByRole("navigation", { name: "VPN sections" })).getByRole("button", { name }),
  );
  return user;
}

describe("VpnPage", () => {
  it("says so when no client is installed", () => {
    render(<VpnPage onNavigate={vi.fn()} />);
    expect(screen.getByText("No VPN clients detected")).toBeInTheDocument();
  });

  it("warns when two clients are connected at once", () => {
    vpn.statuses = [status("tailscale", true, "connected"), status("twingate", true, "connected")];
    render(<VpnPage onNavigate={vi.fn()} />);
    expect(screen.getByText(/Tailscale and Twingate are connected at the same time/)).toBeInTheDocument();
  });

  it("gives each installed client a tab, and none to the rest", () => {
    vpn.statuses = [status("twingate", false, "not running"), { ...status("netbird", false, ""), installed: false }];
    render(<VpnPage onNavigate={vi.fn()} />);
    const nav = screen.getByRole("navigation", { name: "VPN sections" });
    expect(within(nav).getAllByRole("button").map((b) => b.textContent)).toEqual(["Overview", "Twingate"]);
  });

  it("lists live resources with compact auth and re-auth advice", async () => {
    vpn.statuses = [status("twingate", true, "connected")];
    vpn.resources = [
      resource(),
      resource({ name: "prod", authStatus: "Authentication required", needsAuth: true }),
    ];
    await openTab("Twingate");

    expect(screen.getByText("4 days")).toBeInTheDocument();
    expect(screen.getByText("required")).toBeInTheDocument();
    expect(screen.getByText('twingate auth "prod"')).toBeInTheDocument();
    expect(screen.queryByText(/is not answering/)).not.toBeInTheDocument();
  });

  it("labels a remembered list and does not advise acting on its old auth", async () => {
    vpn.statuses = [status("twingate", false, "not running")];
    vpn.resources = [resource({ name: "prod", authStatus: "Authentication required", needsAuth: true })];
    vpn.resourcesSeenAt = "2026-09-10T08:00:00Z";
    await openTab("Twingate");

    expect(screen.getByText(/Twingate is not answering, so this is the list it last reported/)).toBeInTheDocument();
    expect(screen.getByText("192.168.9.0/24")).toBeInTheDocument();
    expect(screen.queryByText(/twingate auth/)).not.toBeInTheDocument();
  });

  it("shows which saved hosts go through a client and opens one", async () => {
    vpn.statuses = [status("twingate", false, "not running")];
    hosts = [hostRow({ id: "db", label: "db-1", hostname: "192.168.9.20" }), hostRow({ id: "x", hostname: "10.1.1.1" })];
    vpn.bindings = {
      "192.168.9.20": { hostname: "192.168.9.20", kind: "twingate", description: "Twingate resource 'acme lab'" },
    };
    const onNavigate = vi.fn();
    const user = userEvent.setup();
    render(<VpnPage onNavigate={onNavigate} />);

    expect(screen.getByText(/not running · 1 host/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /db-1/ }));
    expect(onNavigate).toHaveBeenCalledWith({ kind: "host", hostId: "db" });
    expect(screen.queryByText("10.1.1.1")).not.toBeInTheDocument();
  });
});
