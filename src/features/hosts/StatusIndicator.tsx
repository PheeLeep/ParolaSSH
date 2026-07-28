import { useEffect, useRef, useState } from "react";
import { Link, Unlink } from "lucide-react";
import { STATUS_LABELS, type HostStatus } from "./types";

/** How long the pulse class stays on - must outlast the CSS animation. */
const PULSE_MS = 950;

/** Whole chain only while a session is open; broken for every other state,
 *  since "reachable" still means we are not on the box. */
function Chain({ status }: { status: HostStatus }) {
  const linked = status === "connected";
  const Glyph = linked ? Link : Unlink;

  return (
    <Glyph
      className={`status-mark__chain${linked ? " status-mark__chain--linked" : ""}`}
      aria-hidden="true"
    />
  );
}

function Dot({ status, pulsing }: { status: HostStatus; pulsing: boolean }) {
  return (
    <span
      className={`status-dot status-dot--${status}${
        pulsing ? " status-dot--pulse" : ""
      }`}
      aria-hidden="true"
    />
  );
}

/**
 * True for one pulse cycle after `status` changes, false on first render.
 *
 * Host status is driven by a 30s background heartbeat, so without this a
 * host going offline is a silent colour swap that nobody is looking at.
 */
function useStatusPulse(status: HostStatus) {
  const [pulsing, setPulsing] = useState(false);
  const previous = useRef<HostStatus | null>(null);

  useEffect(() => {
    // Skip the mount pass: every dot flashing on load is noise, not signal.
    if (previous.current === null) {
      previous.current = status;
      return;
    }
    if (previous.current === status) return;

    previous.current = status;
    setPulsing(true);
    const timer = window.setTimeout(() => setPulsing(false), PULSE_MS);
    return () => window.clearTimeout(timer);
  }, [status]);

  return pulsing;
}

/** Chain says whether we hold a session, dot says whether the host answered
 *  its last probe. One mark could not carry both. */
export function StatusDot({
  status,
  title,
}: {
  status: HostStatus;
  title?: string;
}) {
  const pulsing = useStatusPulse(status);
  const label = title ?? STATUS_LABELS[status];

  return (
    <span className="status-mark" role="img" aria-label={label} title={label}>
      <Chain status={status} />
      <Dot status={status} pulsing={pulsing} />
    </span>
  );
}

/** Dot plus label, tinted rather than filled - quieter than a solid badge. */
export function StatusBadge({ status }: { status: HostStatus }) {
  const pulsing = useStatusPulse(status);

  return (
    <span className={`status-badge status-badge--${status}`}>
      <span className="status-mark" aria-hidden="true">
        <Chain status={status} />
        <Dot status={status} pulsing={pulsing} />
      </span>
      {STATUS_LABELS[status]}
    </span>
  );
}
