import { STATUS_LABELS, type HostStatus } from "./types";

/** Small coloured node — online gets a soft halo so it reads at a glance. */
export function StatusDot({
  status,
  title,
}: {
  status: HostStatus;
  title?: string;
}) {
  return (
    <span
      className={`status-dot status-dot--${status}`}
      role="img"
      aria-label={title ?? STATUS_LABELS[status]}
      title={title ?? STATUS_LABELS[status]}
    />
  );
}

/** Dot plus label, tinted rather than filled — quieter than a solid badge. */
export function StatusBadge({ status }: { status: HostStatus }) {
  return (
    <span className={`status-badge status-badge--${status}`}>
      <span className={`status-dot status-dot--${status}`} aria-hidden="true" />
      {STATUS_LABELS[status]}
    </span>
  );
}
