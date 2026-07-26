import { useEffect, useId, useRef, useState } from "react";
import { Alert, Card, ProgressBar, Spinner } from "react-bootstrap";
import { Clock, Cpu, Gauge, MemoryStick } from "lucide-react";
import { Segmented } from "../../../components/Segmented";
import * as api from "../api";
import { errorMessage } from "../api";
import type { HostMetrics } from "../types";

/** Offered cadences. The pane polls only while mounted and visible either
 *  way — deliberately not the 30-second heartbeat, which answers "is it
 *  up?" and is uselessly coarse for watching a load spike. */
const INTERVALS = [
  { value: "1", label: "1 s" },
  { value: "2", label: "2 s" },
  { value: "5", label: "5 s" },
  { value: "10", label: "10 s" },
  { value: "30", label: "30 s" },
] as const;

type IntervalChoice = (typeof INTERVALS)[number]["value"];

const DEFAULT_INTERVAL: IntervalChoice = "1";

/** How many samples the sparklines keep. */
const HISTORY_LIMIT = 60;

/** Where the CPU trace turns red. Sustained load above this is the point at
 *  which the box has nothing left to give, so it should read as a warning
 *  without anyone having to look at the number. */
const CPU_HOT_PERCENT = 80;

export function PerformancePane({ hostId }: { hostId: string }) {
  const [metrics, setMetrics] = useState<HostMetrics | null>(null);
  const [history, setHistory] = useState<HostMetrics[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [interval, setInterval] = useState<IntervalChoice>(DEFAULT_INTERVAL);

  // History resets with the host: samples from one machine say nothing
  // about another.
  useEffect(() => {
    setMetrics(null);
    setHistory([]);
    setError(null);
  }, [hostId]);

  const busyRef = useRef(false);
  useEffect(() => {
    let cancelled = false;

    const beat = async () => {
      // A slow host must not stack samples behind itself.
      if (document.hidden || busyRef.current) return;
      busyRef.current = true;
      try {
        const sample = await api.sampleMetrics(hostId);
        if (cancelled) return;
        setMetrics(sample);
        setHistory((previous) => [...previous, sample].slice(-HISTORY_LIMIT));
        setError(null);
      } catch (caught) {
        if (!cancelled) setError(errorMessage(caught));
      } finally {
        busyRef.current = false;
      }
    };

    void beat();
    const timer = window.setInterval(() => void beat(), Number(interval) * 1000);
    const onVisibility = () => {
      if (!document.hidden) void beat();
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [hostId, interval]);

  if (error && !metrics) {
    return <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>;
  }

  if (!metrics) {
    return (
      <div className="d-flex align-items-center gap-2 text-body-secondary py-4">
        <Spinner animation="border" size="sm" aria-hidden="true" />
        Taking the first sample…
      </div>
    );
  }

  const cpuHistory = history
    .map((sample) => sample.cpuPercent)
    .filter((value): value is number => value !== null);

  return (
    <div className="d-flex flex-column gap-3">
      <div className="d-flex align-items-center gap-2">
        <span className="text-body-secondary small me-auto">
          Sampled while this pane is open.
        </span>
        <Segmented
          value={interval}
          options={[...INTERVALS]}
          onChange={setInterval}
          label="Sampling interval"
        />
      </div>

      {error && <Alert variant="warning" className="text-prewrap mb-0">{error}</Alert>}

      <div className="stat-grid">
        <div className="stat-tile">
          <div className="stat-tile__label">
            <Cpu className="stat-tile__glyph" aria-hidden="true" />
            CPU
          </div>
          <div className="stat-tile__value">
            {metrics.cpuPercent !== null ? `${Math.round(metrics.cpuPercent)} %` : "—"}
          </div>
          <div className="stat-tile__sub">
            {cpuHistory.length > 1 ? (
              <Sparkline
                values={cpuHistory}
                max={100}
                hot={cpuHistory[cpuHistory.length - 1] >= CPU_HOT_PERCENT}
              />
            ) : (
              `sampled every ${interval} s`
            )}
          </div>
        </div>

        <div className="stat-tile">
          <div className="stat-tile__label">
            <MemoryStick className="stat-tile__glyph" aria-hidden="true" />
            Memory
          </div>
          <div className="stat-tile__value">
            {metrics.memory ? `${Math.round(metrics.memory.usedPercent)} %` : "—"}
          </div>
          {metrics.memory && (
            <div className="stat-tile__sub">
              {formatKb(metrics.memory.totalKb - metrics.memory.availableKb)} of{" "}
              {formatKb(metrics.memory.totalKb)}
            </div>
          )}
        </div>

        <div className="stat-tile">
          <div className="stat-tile__label">
            <Gauge className="stat-tile__glyph" aria-hidden="true" />
            Load
          </div>
          <div className="stat-tile__value font-monospace">
            {metrics.load ? metrics.load.map((v) => v.toFixed(2)).join(" ") : "—"}
          </div>
          <div className="stat-tile__sub">1 / 5 / 15 minutes</div>
        </div>

        <div className="stat-tile">
          <div className="stat-tile__label">
            <Clock className="stat-tile__glyph" aria-hidden="true" />
            Uptime
          </div>
          <div className="stat-tile__value">
            {metrics.uptimeSeconds !== null ? formatUptime(metrics.uptimeSeconds) : "—"}
          </div>
        </div>
      </div>

      {metrics.disks.length > 0 && (
        <Card>
          <Card.Body>
            <h2 className="h6 mb-3">Disks</h2>
            <div className="d-flex flex-column gap-3">
              {metrics.disks.map((disk) => (
                <div key={disk.mount}>
                  <div className="d-flex justify-content-between small mb-1">
                    <code>{disk.mount}</code>
                    <span className="text-body-secondary">
                      {formatKb(disk.usedKb)} of {formatKb(disk.totalKb)} ·{" "}
                      {disk.usedPercent.toFixed(0)} %
                    </span>
                  </div>
                  <ProgressBar
                    now={disk.usedPercent}
                    variant={
                      disk.usedPercent >= 90
                        ? "danger"
                        : disk.usedPercent >= 75
                          ? "warning"
                          : undefined
                    }
                    style={{ height: "0.375rem" }}
                    aria-label={`${disk.mount} usage`}
                  />
                </div>
              ))}
            </div>
          </Card.Body>
        </Card>
      )}

      {metrics.notes.length > 0 && (
        <div className="text-body-secondary small">
          {metrics.notes.map((note) => (
            <div key={note}>{note}</div>
          ))}
        </div>
      )}
    </div>
  );
}

/** A tiny inline history line — no chart library for four data series. The
 *  box is stretched to the tile width, so the drawing runs in fixed viewBox
 *  units and the stroke opts out of the scaling. */
const SPARK_WIDTH = 120;
const SPARK_HEIGHT = 28;
/** Keeps the stroke off the edges, where half of it would be clipped. */
const SPARK_PAD = 2;

type Point = { x: number; y: number };

function Sparkline({
  values,
  max,
  hot,
}: {
  values: number[];
  max: number;
  hot: boolean;
}) {
  // Colons out of React's id: this goes in a `url(#…)` reference.
  const gradientId = `spark-${useId().replace(/:/g, "")}`;

  const step = SPARK_WIDTH / (values.length - 1);
  const span = SPARK_HEIGHT - SPARK_PAD * 2;
  const points: Point[] = values.map((value, index) => ({
    x: index * step,
    y: SPARK_PAD + (1 - clamp(value, 0, max) / max) * span,
  }));

  const line = smoothPath(points);
  const last = points[points.length - 1];
  const area = `${line} L ${last.x.toFixed(1)},${SPARK_HEIGHT} L ${points[0].x.toFixed(
    1,
  )},${SPARK_HEIGHT} Z`;

  return (
    <svg
      className={`sparkline${hot ? " is-hot" : ""}`}
      viewBox={`0 0 ${SPARK_WIDTH} ${SPARK_HEIGHT}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={`CPU history, last ${values.length} samples`}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop className="sparkline__stop--top" offset="0%" />
          <stop className="sparkline__stop--bottom" offset="100%" />
        </linearGradient>
      </defs>
      <path className="sparkline__area" d={area} fill={`url(#${gradientId})`} />
      <path className="sparkline__line" d={line} />
    </svg>
  );
}

/**
 * Catmull-Rom through every sample, emitted as cubic Béziers.
 *
 * Control points are clamped to the box: a spike between two low samples
 * otherwise overshoots past 100 % and draws load the host never had.
 */
function smoothPath(points: Point[]): string {
  const top = SPARK_PAD;
  const bottom = SPARK_HEIGHT - SPARK_PAD;
  let d = `M ${points[0].x.toFixed(1)},${points[0].y.toFixed(1)}`;

  for (let i = 0; i < points.length - 1; i += 1) {
    const previous = points[i - 1] ?? points[i];
    const start = points[i];
    const end = points[i + 1];
    const next = points[i + 2] ?? end;

    const c1x = start.x + (end.x - previous.x) / 6;
    const c1y = clamp(start.y + (end.y - previous.y) / 6, top, bottom);
    const c2x = end.x - (next.x - start.x) / 6;
    const c2y = clamp(end.y - (next.y - start.y) / 6, top, bottom);

    d += ` C ${c1x.toFixed(1)},${c1y.toFixed(1)} ${c2x.toFixed(1)},${c2y.toFixed(
      1,
    )} ${end.x.toFixed(1)},${end.y.toFixed(1)}`;
  }

  return d;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}

function formatKb(kb: number): string {
  const mib = kb / 1024;
  if (mib < 1024) return `${mib.toFixed(0)} MiB`;
  const gib = mib / 1024;
  if (gib < 1024) return `${gib.toFixed(1)} GiB`;
  return `${(gib / 1024).toFixed(2)} TiB`;
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  if (days > 0) return `${days} d ${hours} h`;
  if (hours > 0) return `${hours} h ${minutes} m`;
  return `${minutes} m`;
}
