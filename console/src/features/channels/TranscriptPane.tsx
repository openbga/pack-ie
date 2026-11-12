import { type UIEvent, useId, useLayoutEffect, useRef, useState } from "react";
import { Checkbox } from "@/components/Checkbox";
import { formatCount } from "@/lib/format";
import styles from "./TrafficPanel.module.css";
import { SegmentRow } from "./TrafficSegment";
import { flowMarker } from "./trafficText";
import { MAX_SEGMENTS, type TrafficEntry } from "./useTrafficFeed";

/** Within this many pixels of the end, the pane counts as scrolled to the end. */
const AT_END_PX = 24;

const segmentsText = (count: number) =>
  `${formatCount(count)} ${count === 1 ? "segment" : "segments"}`;

/** The label of the destination a segment is tagged with (its id when unknown); null when untagged. */
function destinationTag(
  entry: TrafficEntry,
  labels: ReadonlyMap<string, string> | null,
): string | null {
  const id = entry.segment.destinationId ?? null;
  if (labels === null || id === null) return null;
  return labels.get(id) ?? id;
}

/**
 * One direction of the transcript: its own heading, count, autoscroll and scrolling log.
 * Autoscroll follows new segments until the operator scrolls up, and resumes on request.
 */
export function TranscriptPane({
  title,
  entries,
  received,
  showHex,
  clock,
  isFrozen,
  isActive,
  destinationLabels,
}: {
  title: string;
  entries: readonly TrafficEntry[];
  /** Segments received since Clear, including those that have left the pane. */
  received: number;
  showHex: boolean;
  clock: (at: string) => string;
  isFrozen: boolean;
  /** The transcript is on, or has something to show. */
  isActive: boolean;
  /**
   * Message channels: destination id to label. Its presence turns on direction markers and
   * destination tags; null for a bridge, whose panes already say which way they go.
   */
  destinationLabels: ReadonlyMap<string, string> | null;
}) {
  const headingId = useId();
  const logRef = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `entries` is the trigger: scroll again after each batch renders.
  useLayoutEffect(() => {
    const log = logRef.current;
    if (follow && log) log.scrollTop = log.scrollHeight;
  }, [follow, entries]);
  function onScroll(event: UIEvent<HTMLDivElement>) {
    const log = event.currentTarget;
    const atEnd = log.scrollHeight - log.scrollTop - log.clientHeight <= AT_END_PX;
    if (atEnd !== follow) setFollow(atEnd);
  }
  return (
    <section aria-labelledby={headingId} className={styles.pane}>
      <div className={styles.paneHeader}>
        <h3 id={headingId} className={styles.paneTitle}>
          {title}
        </h3>
        <span className={styles.count}>
          {isActive &&
            (received > entries.length
              ? `Newest ${formatCount(entries.length)} of ${formatCount(received)} segments`
              : segmentsText(entries.length))}
        </span>
        <Checkbox isSelected={follow} onChange={setFollow} aria-label={`Autoscroll ${title}`}>
          Autoscroll
        </Checkbox>
      </div>
      <div
        ref={logRef}
        role="log"
        aria-live="off"
        aria-label={`Transcript ${title}`}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrolling region must be reachable by keyboard to be read (WCAG 2.1.1).
        tabIndex={0}
        className={styles.log}
        data-frozen={isFrozen || undefined}
        onScroll={onScroll}
      >
        {entries.length === 0 ? (
          <p className={styles.waiting}>
            {isActive ? "Waiting for traffic…" : "Nothing captured. Start the transcript to watch."}
          </p>
        ) : (
          <ol className={styles.list}>
            {entries.map((entry) => (
              <SegmentRow
                key={entry.key}
                entry={entry}
                time={clock(entry.segment.at)}
                showHex={showHex}
                marker={destinationLabels === null ? null : flowMarker(entry.segment)}
                tag={destinationTag(entry, destinationLabels)}
              />
            ))}
          </ol>
        )}
      </div>
      <p className={styles.limit}>Keeps the newest {formatCount(MAX_SEGMENTS)} segments.</p>
    </section>
  );
}
