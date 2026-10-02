export type LogLevel = "error" | "warn" | "info" | "debug" | "none";

export interface LogRow {
  /** Original timestamp text, for the hover title. */
  stamp: string;
  /** Short time of day, or "" for rows without a timestamp. */
  time: string;
  level: LogLevel;
  /** Process or event source, e.g. "sshd[123]" or "7036". */
  source: string;
  message: string;
}

// journald short-iso: "2026-10-01T14:40:06+08:00 host ident[pid]: message"
const JOURNAL = /^(\d{4}-\d\d-\d\dT[\d:.]+(?:[+-]\d\d:?\d\d|Z)?)\s+\S+\s+([^\s:]+):\s?(.*)$/;
// SCM text from wevtutil: "2026-07-20T10:15:30.123  [7036]  message"
const SCM = /^(\d{4}-\d\d-\d\dT[\d:.]+)\s+\[(\d+)\]\s+(.*)$/;

const BRACKET_LEVELS: Record<string, LogLevel> = {
  ftl: "error", fatal: "error", crit: "error", err: "error", error: "error",
  wrn: "warn", warn: "warn", warning: "warn",
  inf: "info", info: "info",
  dbg: "debug", debug: "debug", trc: "debug", trace: "debug",
};

/** Level from an explicit `[INF]`-style tag, else from telltale words. */
export function detectLevel(message: string): LogLevel {
  const tag = /^\s*\[?(ftl|fatal|crit|err|error|wrn|warn|warning|inf|info|dbg|debug|trc|trace)\]/i.exec(
    message,
  );
  if (tag) return BRACKET_LEVELS[tag[1].toLowerCase()];
  if (/\b(error|failed|failure|fatal|exception|panic|terminated unexpectedly)\b/i.test(message)) {
    return "error";
  }
  if (/\b(warn|warning|deprecated)\b/i.test(message)) return "warn";
  return "none";
}

/** Parses journal or SCM text. Lines without a timestamp continue the previous
 *  row (wrapped stack traces), or stand alone when nothing precedes them. */
export function parseServiceLog(lines: string[]): LogRow[] {
  const rows: LogRow[] = [];
  for (const raw of lines) {
    const line = raw.replace(/\r$/, "");
    if (!line.trim()) continue;

    const scm = SCM.exec(line);
    if (scm) {
      const id = scm[2];
      const level: LogLevel = id === "7031" || id === "7034" ? "error" : "info";
      rows.push({ stamp: scm[1], time: shortTime(scm[1]), level, source: id, message: scm[3] });
      continue;
    }

    const journal = JOURNAL.exec(line);
    if (journal) {
      rows.push({
        stamp: journal[1],
        time: shortTime(journal[1]),
        level: detectLevel(journal[3]),
        source: journal[2],
        message: journal[3],
      });
      continue;
    }

    const last = rows[rows.length - 1];
    if (last && last.time && !line.startsWith("--")) last.message += `\n${line}`;
    else rows.push({ stamp: "", time: "", level: "none", source: "", message: line });
  }
  return rows;
}

function shortTime(stamp: string): string {
  return /T(\d\d:\d\d:\d\d)/.exec(stamp)?.[1] ?? stamp;
}
