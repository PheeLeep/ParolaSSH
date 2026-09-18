import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Alert, Badge, Card, Spinner } from "react-bootstrap";
import {
  ArrowDown,
  ArrowUp,
  Clock,
  Cpu,
  Gauge,
  HardDriveDownload,
  HardDriveUpload,
  MemoryStick,
} from "lucide-react";
import { Segmented } from "../../../components/Segmented";
import { useStoreSubscription } from "../../../lib/useStoreSubscription";
import * as api from "../api";
import { errorMessage } from "../api";
import * as metricsCache from "../metricsCache";
import type { IntervalChoice } from "../metricsCache";
import type { DiskIo, NetworkRate } from "../types";

/** Offered cadences. The pane polls only while mounted and visible either
 *  way - deliberately not the 30-second heartbeat, which answers "is it
 *  up?" and is uselessly coarse for watching a load spike. */
const INTERVALS: { value: IntervalChoice; label: string }[] = [
  {value: "0.5", label: "500ms"},
  { value: "1", label: "1s" },
  { value: "2", label: "2s" },
  { value: "5", label: "5s" },
  { value: "10", label: "10s" },
  { value: "30", label: "30s" },
];

/** Where the CPU trace turns red. Sustained load above this is the point at
 *  which the box has nothing left to give, so it should read as a warning
 *  without anyone having to look at the number. */
const CPU_HOT_PERCENT = 80;

/** Memory past this is swapping territory on most hosts. */
const MEMORY_HOT_PERCENT = 90;

/** A disk turns red with a quarter of its space left. */
const DISK_HOT_PERCENT = 75;

export function PerformancePane({ hostId }: { hostId: string }) {
  useStoreSubscription(metricsCache.subscribe);
  const history = metricsCache.history(hostId);
  const interval = metricsCache.interval(hostId);
  const metrics = history.length > 0 ? history[history.length - 1] : null;
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
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
        metricsCache.push(hostId, sample);
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
  const memoryHistory = history
    .map((sample) => sample.memory?.usedPercent)
    .filter((value): value is number => value !== undefined);
  const netHistory = history
    .map((sample) => sample.network)
    .filter((value): value is NetworkRate => value !== null);
  const rxHistory = netHistory.map((rate) => rate.rxBytesPerSec);
  const txHistory = netHistory.map((rate) => rate.txBytesPerSec);
  // One scale for both directions, so a quiet uplink reads as quiet.
  const netMax = Math.max(1, ...rxHistory, ...txHistory);
  const interfaces = metrics.network?.interfaces ?? [];
  const ioHistory = history
    .map((sample) => sample.diskIo)
    .filter((value): value is DiskIo => value !== null);
  const readHistory = ioHistory.map((io) => io.readBytesPerSec);
  const writeHistory = ioHistory.map((io) => io.writeBytesPerSec);
  const ioMax = Math.max(1, ...readHistory, ...writeHistory);
  const devices = metrics.diskIo?.devices ?? [];

  return (
    <div className="d-flex flex-column gap-3">
      <div className="d-flex align-items-center justify-content-end gap-2">
        <span className="text-body-secondary small">Interval</span>
        <Segmented
          value={interval}
          options={INTERVALS}
          onChange={(choice) => metricsCache.setInterval(hostId, choice)}
          label="Sampling interval"
        />
      </div>

      {error && <Alert variant="warning" className="text-prewrap mb-0">{error}</Alert>}

      <Section title="CPU & memory">
        <div className="stat-grid">
          <div className="stat-tile">
            <div className="stat-tile__label">
              <Cpu className="stat-tile__glyph" aria-hidden="true" />
              CPU
            </div>
            <div className="stat-tile__value">
              {metrics.cpuPercent !== null ? `${Math.round(metrics.cpuPercent)} %` : "-"}
            </div>
            <div className="stat-tile__sub">
              <Sparkline
                values={cpuHistory}
                max={100}
                hot={cpuHistory[cpuHistory.length - 1] >= CPU_HOT_PERCENT}
                label="CPU history"
              />
            </div>
          </div>

          <div className="stat-tile">
            <div className="stat-tile__label">
              <MemoryStick className="stat-tile__glyph" aria-hidden="true" />
              Memory
            </div>
            <div className="stat-tile__value">
              {metrics.memory ? `${Math.round(metrics.memory.usedPercent)} %` : "-"}
            </div>
            {metrics.memory && (
              <div className="stat-tile__sub">
                {formatKb(metrics.memory.totalKb - metrics.memory.availableKb)} of{" "}
                {formatKb(metrics.memory.totalKb)}
                <Sparkline
                  values={memoryHistory}
                  max={100}
                  hot={metrics.memory.usedPercent >= MEMORY_HOT_PERCENT}
                  label="Memory history"
                />
              </div>
            )}
          </div>

          <div className="stat-tile">
            <div className="stat-tile__label">
              <Gauge className="stat-tile__glyph" aria-hidden="true" />
              Load
            </div>
            <div className="stat-tile__value font-monospace">
              {metrics.load ? metrics.load.map((v) => v.toFixed(2)).join(" ") : "-"}
            </div>
            <div className="stat-tile__sub">1 / 5 / 15 minutes</div>
          </div>

          <div className="stat-tile">
            <div className="stat-tile__label">
              <Clock className="stat-tile__glyph" aria-hidden="true" />
              Uptime
            </div>
            <div className="stat-tile__value">
              {metrics.uptimeSeconds !== null ? formatUptime(metrics.uptimeSeconds) : "-"}
            </div>
          </div>
        </div>
      </Section>

      <Section title="Network">
        <div className="stat-grid">
          <div className="stat-tile">
            <div className="stat-tile__label">
              <ArrowDown className="stat-tile__glyph" aria-hidden="true" />
              Downlink
            </div>
            <div className="stat-tile__value">
              {metrics.network ? formatBitRate(metrics.network.rxBytesPerSec) : "-"}
            </div>
            <div className="stat-tile__sub">
              <Sparkline values={rxHistory} max={netMax} label="Downlink history" />
            </div>
          </div>

          <div className="stat-tile">
            <div className="stat-tile__label">
              <ArrowUp className="stat-tile__glyph" aria-hidden="true" />
              Uplink
            </div>
            <div className="stat-tile__value">
              {metrics.network ? formatBitRate(metrics.network.txBytesPerSec) : "-"}
            </div>
            <div className="stat-tile__sub">
              <Sparkline values={txHistory} max={netMax} label="Uplink history" />
            </div>
          </div>
        </div>

        {/* One physical interface is already the totals above. */}
        {(interfaces.length > 1 || interfaces.some((entry) => entry.isVirtual)) && (
          <div className="io-list mt-3">
            {interfaces.map((entry) => (
              <div key={entry.name} className="io-list__row">
                <code className="text-truncate">{entry.name}</code>
                {entry.isVirtual && (
                  <Badge
                    bg="secondary"
                    className="fw-normal"
                    title="Relays traffic a physical interface also carries, so it is left out of the totals"
                  >
                    Virtual
                  </Badge>
                )}
                <span className="io-list__rate ms-auto">
                  <ArrowDown className="icon-sm" aria-label="Down" />
                  {formatBitRate(entry.rxBytesPerSec)}
                </span>
                <span className="io-list__rate">
                  <ArrowUp className="icon-sm" aria-label="Up" />
                  {formatBitRate(entry.txBytesPerSec)}
                </span>
              </div>
            ))}
          </div>
        )}
      </Section>

      {(metrics.disks.length > 0 || ioHistory.length > 0) && (
        <Section title="Disks">
          <div className="stat-grid">
            <div className="stat-tile">
              <div className="stat-tile__label">
                <HardDriveDownload className="stat-tile__glyph" aria-hidden="true" />
                Read
              </div>
              <div className="stat-tile__value">
                {metrics.diskIo ? formatByteRate(metrics.diskIo.readBytesPerSec) : "-"}
              </div>
              <div className="stat-tile__sub">
                <Sparkline values={readHistory} max={ioMax} label="Disk read history" />
              </div>
            </div>

            <div className="stat-tile">
              <div className="stat-tile__label">
                <HardDriveUpload className="stat-tile__glyph" aria-hidden="true" />
                Write
              </div>
              <div className="stat-tile__value">
                {metrics.diskIo ? formatByteRate(metrics.diskIo.writeBytesPerSec) : "-"}
              </div>
              <div className="stat-tile__sub">
                <Sparkline values={writeHistory} max={ioMax} label="Disk write history" />
              </div>
            </div>
          </div>

          {devices.length > 1 && (
            <div className="io-list mt-3">
              {devices.map((device) => (
                <div key={device.name} className="io-list__row">
                  <code className="text-truncate">{device.name}</code>
                  <span className="io-list__rate ms-auto">
                    <span className="text-body-secondary">R</span>
                    {formatByteRate(device.readBytesPerSec)}
                  </span>
                  <span className="io-list__rate">
                    <span className="text-body-secondary">W</span>
                    {formatByteRate(device.writeBytesPerSec)}
                  </span>
                </div>
              ))}
            </div>
          )}

          <div className="d-flex flex-column gap-3 mt-3">
            {metrics.disks.map((disk) => (
              <div key={disk.mount}>
                <div className="d-flex justify-content-between small mb-1">
                  <code>{disk.mount}</code>
                  <span className="text-body-secondary">
                    {formatKb(disk.usedKb)} of {formatKb(disk.totalKb)} ·{" "}
                    {disk.usedPercent.toFixed(0)} %
                  </span>
                </div>
                <div
                  className={`usage-bar${disk.usedPercent >= DISK_HOT_PERCENT ? " is-hot" : ""}`}
                  role="progressbar"
                  aria-label={`${disk.mount} usage`}
                  aria-valuenow={Math.round(disk.usedPercent)}
                  aria-valuemin={0}
                  aria-valuemax={100}
                >
                  <div
                    className="usage-bar__fill"
                    style={{ width: `${clamp(disk.usedPercent, 0, 100)}%` }}
                  />
                </div>
              </div>
            ))}
          </div>
        </Section>
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

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card>
      <Card.Body>
        <h2 className="h6 mb-3">{title}</h2>
        {children}
      </Card.Body>
    </Card>
  );
}

/** A tiny inline history line - no chart library for a handful of series. The
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
  hot = false,
  label,
}: {
  values: number[];
  max: number;
  hot?: boolean;
  label: string;
}) {
  // Colons out of React's id: this goes in a `url(#…)` reference.
  const gradientId = `spark-${useId().replace(/:/g, "")}`;

  // The chart shows from the start: empty until a reading, flat for one.
  const series = values.length === 1 ? [values[0], values[0]] : values;
  const step = SPARK_WIDTH / (series.length - 1);
  const span = SPARK_HEIGHT - SPARK_PAD * 2;
  const points: Point[] = series.map((value, index) => ({
    x: index * step,
    y: SPARK_PAD + (1 - clamp(value, 0, max) / max) * span,
  }));

  let line = "";
  let area = "";
  if (points.length > 1) {
    line = smoothPath(points);
    const last = points[points.length - 1];
    area = `${line} L ${last.x.toFixed(1)},${SPARK_HEIGHT} L ${points[0].x.toFixed(
      1,
    )},${SPARK_HEIGHT} Z`;
  }

  return (
    <svg
      className={`sparkline${hot ? " is-hot" : ""}`}
      viewBox={`0 0 ${SPARK_WIDTH} ${SPARK_HEIGHT}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={`${label}, last ${values.length} samples`}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop className="sparkline__stop--top" offset="0%" />
          <stop className="sparkline__stop--bottom" offset="100%" />
        </linearGradient>
      </defs>
      {line && (
        <>
          <path className="sparkline__area" d={area} fill={`url(#${gradientId})`} />
          <path className="sparkline__line" d={line} />
        </>
      )}
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

/** Link speeds are quoted in bits, so the meter is too. */
function formatBitRate(bytesPerSec: number): string {
  const bits = bytesPerSec * 8;
  if (bits < 1_000) return `${bits.toFixed(0)} bps`;
  if (bits < 1_000_000) return `${(bits / 1_000).toFixed(1)} kbps`;
  if (bits < 1_000_000_000) return `${(bits / 1_000_000).toFixed(1)} Mbps`;
  return `${(bits / 1_000_000_000).toFixed(2)} Gbps`;
}

/** Disk throughput is quoted in bytes, binary units like the sizes above. */
function formatByteRate(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  const kib = bytesPerSec / 1024;
  if (kib < 1024) return `${kib.toFixed(1)} KiB/s`;
  const mib = kib / 1024;
  if (mib < 1024) return `${mib.toFixed(1)} MiB/s`;
  return `${(mib / 1024).toFixed(2)} GiB/s`;
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  if (days > 0) return `${days} d ${hours} h`;
  if (hours > 0) return `${hours} h ${minutes} m`;
  return `${minutes} m`;
}
