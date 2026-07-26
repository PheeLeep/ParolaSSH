import type { HostFeature } from "./features/hosts/HostFeatureNav";

/** Which pane the main area is showing. Deliberately not a router yet —
 *  swap for TanStack Router once sessions need deep links. */
export type View =
  | { kind: "welcome" }
  | { kind: "hosts" }
  /** `feature` and `shellId` let Sessions land on one specific shell tab. */
  | { kind: "host"; hostId: string; feature?: HostFeature; shellId?: number }
  | { kind: "keys" }
  | { kind: "key"; keyId: string }
  | { kind: "audit" }
  | { kind: "sessions" }
  | { kind: "vpn" }
  | { kind: "settings" }
  | { kind: "about" };

export type Navigate = (view: View) => void;
