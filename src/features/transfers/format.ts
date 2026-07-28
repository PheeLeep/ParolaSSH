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

/** Whole percent, clamped - a server that reports a stale size can otherwise
 *  push a progress bar past its own end. */
export function percentOf(done: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((done / total) * 100)));
}
