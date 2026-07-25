import { useEffect, useRef, useState } from "react";
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
              <Sparkline values={cpuHistory} max={100} />
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

/** A tiny inline history line — no chart library for four data series. */
function Sparkline({ values, max }: { values: number[]; max: number }) {
  const width = 120;
  const height = 24;
  const step = values.length > 1 ? width / (values.length - 1) : width;
  const points = values
    .map((value, index) => {
      const x = index * step;
      const y = height - (Math.min(value, max) / max) * height;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label="CPU history"
    >
      <polyline
        points={points}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
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
