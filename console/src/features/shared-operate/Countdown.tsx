import { countdownText } from "./countdownText";
import { useNow } from "./useNow";

/** A live countdown to a scheduled time: `in 5 s`, counting down every second. */
export function Countdown({ until }: { until: string }) {
  const now = useNow();
  return <time dateTime={until}>{countdownText(until, now)}</time>;
}
