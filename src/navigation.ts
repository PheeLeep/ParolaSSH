/** Which pane the main area is showing. Deliberately not a router yet —
 *  swap for TanStack Router once sessions need deep links. */
export type View =
  | { kind: "welcome" }
  | { kind: "hosts" }
  | { kind: "host"; hostId: string };

export type Navigate = (view: View) => void;
