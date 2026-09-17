import {
  Boxes,
  Cable,
  FolderOpen,
  Gauge,
  ListChecks,
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
  | "security"
  | "files"
  | "tunnels";

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
  { id: "security", label: "Security", Icon: ShieldCheck, ready: true, needsSession: true },
  { id: "files", label: "Files", Icon: FolderOpen, ready: true, needsSession: true },
  { id: "tunnels", label: "Tunnels", Icon: Cable, ready: true, needsSession: true },
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
      {/* Tabs that need a session are hidden until one exists. */}
      {HOST_FEATURES.filter((feature) => connected || !feature.needsSession).map((feature) => (
        <button
          key={feature.id}
          type="button"
          className={`feature-nav__item${active === feature.id ? " is-active" : ""}`}
          onClick={() => onSelect(feature.id)}
          aria-current={active === feature.id ? "page" : undefined}
        >
          <feature.Icon className="icon-sm" aria-hidden="true" />
          {feature.label}
          {!feature.ready && <span className="feature-nav__soon">soon</span>}
        </button>
      ))}
    </nav>
  );
}
