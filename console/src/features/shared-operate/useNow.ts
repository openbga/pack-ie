import { useEffect, useState } from "react";

/**
 * The current time in milliseconds, refreshed every `intervalMs` (one second by default), so
 * a countdown or "8 s ago" keeps moving between snapshots.
 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  // Synchronizes with the clock, which lives outside React.
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}
