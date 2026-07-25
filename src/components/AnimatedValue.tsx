import { useEffect, useRef, useState } from "react";

/** Must outlast the `.count-change` CSS animation. */
const FLASH_MS = 320;

/**
 * Briefly flashes when `value` changes, so counts that move on their own —
 * the sidebar's online tallies, updated by the background heartbeat — get
 * noticed instead of silently ticking over.
 */
export function AnimatedValue({ value }: { value: number | string }) {
  const [flashing, setFlashing] = useState(false);
  const previous = useRef<number | string | null>(null);

  useEffect(() => {
    if (previous.current === null) {
      previous.current = value;
      return;
    }
    if (previous.current === value) return;

    previous.current = value;
    setFlashing(true);
    const timer = window.setTimeout(() => setFlashing(false), FLASH_MS);
    return () => window.clearTimeout(timer);
  }, [value]);

  return <span className={flashing ? "count-change" : undefined}>{value}</span>;
}
