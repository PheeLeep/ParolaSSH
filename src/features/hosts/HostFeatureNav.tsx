import {
  Boxes,
  FolderOpen,
  Gauge,
  ListChecks,
  Package,
  Server,
  ShieldCheck,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";

export type HostFeature =
  | "overview"
  | "terminal"
  | "services"
  | "performance"
  | "tasks"
  | "updates"
  | "audit"
  | "files";

type FeatureDef = {
  id: HostFeature;
  label: string;
  Icon: LucideIcon;
  ready: boolean;
  needsSession: boolean;
};


export const HOST_FEATURES: FeatureDef[] = [
  { id: "overview", label: "Overview", Icon: Server, ready: true, needsSession: false },
  { id: "terminal", label: "Terminal", Icon: SquareTerminal, ready: true, needsSession: true },
  { id: "services", label: "Services", Icon: Boxes, ready: true, needsSession: true },
  { id: "performance", label: "Performance", Icon: Gauge, ready: true, needsSession: true },
  { id: "tasks", label: "Tasks", Icon: ListChecks, ready: true, needsSession: true },
  { id: "updates", label: "Updates", Icon: Package, ready: true, needsSession: true },
  { id: "audit", label: "Audit", Icon: ShieldCheck, ready: true, needsSession: true },
  { id: "files", label: "Files", Icon: FolderOpen, ready: true, needsSession: true },
];

export function HostFeatureNav({
  active,
  connected,
  onSelect,
}: {
  active: HostFeature;
  connected: boolean;
  onSelect: (feature: HostFeature) => void;
}) {
  return (
    <nav className="feature-nav" aria-label="Host sections">
      {HOST_FEATURES.map((feature) => {
        const locked = feature.needsSession && !connected;

        return (
          <button
            key={feature.id}
            type="button"
            className={`feature-nav__item${
              active === feature.id ? " is-active" : ""
            }${locked ? " is-locked" : ""}`}
            onClick={() => onSelect(feature.id)}
            aria-current={active === feature.id ? "page" : undefined}
            title={locked ? "Connect to this host first" : undefined}
          >
            <feature.Icon className="icon-sm" aria-hidden="true" />
            {feature.label}
            {!feature.ready && <span className="feature-nav__soon">soon</span>}
          </button>
        );
      })}
    </nav>
  );
}
