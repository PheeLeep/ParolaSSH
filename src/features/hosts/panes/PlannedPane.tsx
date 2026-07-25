import { Card } from "react-bootstrap";
import { FolderOpen, type LucideIcon } from "lucide-react";
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
