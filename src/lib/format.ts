const dateTimeFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

const relativeFormat = new Intl.RelativeTimeFormat(undefined, {
  numeric: "auto",
});

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "3 hours ago" for recent timestamps, an absolute date for older ones. */
export function formatRelative(iso: string | null): string {
  if (!iso) return "Never";

  const timestamp = Date.parse(iso);
  if (Number.isNaN(timestamp)) return "Unknown";

  const elapsed = Date.now() - timestamp;
  if (elapsed < MINUTE) return "Just now";
  if (elapsed < HOUR) return relativeFormat.format(-Math.round(elapsed / MINUTE), "minute");
  if (elapsed < DAY) return relativeFormat.format(-Math.round(elapsed / HOUR), "hour");
  if (elapsed < 7 * DAY) return relativeFormat.format(-Math.round(elapsed / DAY), "day");

  return dateTimeFormat.format(timestamp);
}

export function formatAbsolute(iso: string | null): string {
  if (!iso) return "Never connected";
  const timestamp = Date.parse(iso);
  return Number.isNaN(timestamp) ? "Unknown" : dateTimeFormat.format(timestamp);
}
