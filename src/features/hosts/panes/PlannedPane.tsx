import { Card } from "react-bootstrap";
import { Boxes, FolderOpen, Gauge, Package, ShieldCheck, type LucideIcon } from "lucide-react";
import type { HostFeature } from "../HostFeatureNav";
import type { OsFamily } from "../types";

type Plan = {
  Icon: LucideIcon;
  title: string;
  blurb: string;
  commands: Partial<Record<"unix" | "windows", string[]>>;
  caveat?: string;
};

const PLANS: Partial<Record<HostFeature, Plan>> = {
  services: {
    Icon: Boxes,
    title: "Services",
    blurb:
      "Running units with their state and memory, and start, stop or restart — elevating the same way Power does.",
    commands: {
      unix: ["systemctl list-units --type=service", "journalctl -u <unit> -n 200"],
      windows: ["sc query type= service state= all", "wevtutil qe System /c:50"],
    },
    caveat:
      "Windows has no per-service log: provider names rarely match service names, and most services log to their own files. It will show Service Control Manager start, stop and crash events instead.",
  },
  performance: {
    Icon: Gauge,
    title: "Performance",
    blurb:
      "CPU, memory, disk and load, sampled on the heartbeat you already run rather than on a timer of its own.",
    commands: {
      unix: ["/proc/stat", "/proc/meminfo", "df -P"],
      windows: ["Get-CimInstance Win32_OperatingSystem", "Get-Counter"],
    },
  },
  updates: {
    Icon: Package,
    title: "Updates",
    blurb:
      "Pending packages with security ones marked. Read-only — installing anything stays a decision you make in the terminal.",
    commands: {
      unix: ["apt list --upgradable", "dnf check-update"],
      windows: ["Get-WindowsUpdate"],
    },
  },
  audit: {
    Icon: ShieldCheck,
    title: "Audit",
    blurb:
      "The remote half of the key audit: sshd posture, authorized_keys permissions, empty passwords, world-writable PATH entries.",
    commands: {
      unix: ["sshd -T", "stat ~/.ssh/authorized_keys", "awk -F: '($2==\"\")' /etc/shadow"],
    },
    caveat:
      "Weak ciphers and key exchange come free — russh already negotiated them during the handshake, so that check costs no remote command at all. Lynis stays opt-in and is never installed for you.",
  },
  files: {
    Icon: FolderOpen,
    title: "Files",
    blurb:
      "Browse, upload and download over SFTP, riding the connection you already hold.",
    commands: { unix: ["russh-sftp subsystem"] },
  },
};

export function PlannedPane({
  feature,
  os,
}: {
  feature: HostFeature;
  os?: OsFamily;
}) {
  const plan = PLANS[feature];
  if (!plan) return null;

  // Show the commands for the machine actually in front of you.
  const key = os === "windows" ? "windows" : "unix";
  const commands = plan.commands[key] ?? plan.commands.unix ?? [];

  return (
    <Card>
      <Card.Body className="d-flex gap-3">
        <plan.Icon className="planned__icon flex-shrink-0" aria-hidden="true" />
        <div className="min-w-0">
          <div className="d-flex align-items-center gap-2 mb-1">
            <h2 className="h6 mb-0">{plan.title}</h2>
            <span className="planned__pill">Planned</span>
          </div>

          <p className="text-body-secondary mb-3">{plan.blurb}</p>

          {commands.length > 0 && (
            <>
              <div className="detail-grid__label">
                What it will run{os === "windows" ? " on Windows" : ""}
              </div>
              <ul className="planned__commands">
                {commands.map((command) => (
                  <li key={command}>
                    <code>{command}</code>
                  </li>
                ))}
              </ul>
            </>
          )}

          {plan.caveat && (
            <p className="text-body-secondary small mb-0 mt-3">{plan.caveat}</p>
          )}
        </div>
      </Card.Body>
    </Card>
  );
}
