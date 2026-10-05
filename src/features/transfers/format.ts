/** Number formatting shared by the file browser and the transfer list. */

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Sizes in the units a file manager uses - 1 KB is 1024 B, and the precision
 *  drops as the number grows so a column of them stays the same width. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "-";
  if (bytes < 1024) return `${bytes} B`;

  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return `${value.toFixed(value >= 100 ? 0 : 1)} ${UNITS[unit]}`;
}

/** A transfer speed, or an em dash before there is a measurement to show. */
export function formatSpeed(bytesPerSecond: number | null): string {
  if (bytesPerSecond === null || !Number.isFinite(bytesPerSecond)) return "-";
  return `${formatBytes(Math.round(bytesPerSecond))}/s`;
}

/** A countdown: `45s`, `3m 12s`, `1h 05m`. */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${String(total % 60).padStart(2, "0")}s`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

/** Whole percent, clamped - a server that reports a stale size can otherwise
 *  push a progress bar past its own end. */
export function percentOf(done: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((done / total) * 100)));
}
