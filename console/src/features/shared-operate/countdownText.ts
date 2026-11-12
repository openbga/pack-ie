/**
 * How long until `until` (RFC 3339), as `in 5 s`, `in 2 min 5 s` or `in 1 h 2 min`; `now`
 * once it has passed. Rounds up, so a countdown never reads 0 s before it is due.
 */
export function countdownText(until: string, now: number): string {
  const target = Date.parse(until);
  if (Number.isNaN(target)) return until;
  const seconds = Math.ceil((target - now) / 1000);
  if (seconds <= 0) return "now";
  if (seconds < 60) return `in ${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (seconds < 3600) {
    const rest = seconds % 60;
    return rest === 0 ? `in ${minutes} min` : `in ${minutes} min ${rest} s`;
  }
  return `in ${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}
