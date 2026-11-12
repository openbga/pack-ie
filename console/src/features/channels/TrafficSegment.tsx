import { memo } from "react";
import { formatBytes } from "@/lib/format";
import styles from "./TrafficPanel.module.css";
import { type FlowMarker, toHex, toSymbolicText } from "./trafficText";
import type { TrafficEntry } from "./useTrafficFeed";

interface SegmentRowProps {
  entry: TrafficEntry;
  time: string;
  showHex: boolean;
  /** Message channels: which way the segment went, in an arrow and in words. */
  marker: FlowMarker | null;
  /** Message channels: the destination's label, when the segment is tagged with one. */
  tag: string | null;
}

function SegmentRowView({ entry, time, showHex, marker, tag }: SegmentRowProps) {
  const { segment, bytes } = entry;
  return (
    <li className={styles.segment}>
      <div className={styles.meta}>
        <time dateTime={segment.at} className={styles.time}>
          {time}
        </time>
        {marker && (
          <span className={styles.direction}>
            <span aria-hidden="true">{marker.arrow}</span> {marker.label}
          </span>
        )}
        {tag !== null && <span className={styles.tag}>{tag}</span>}
        <span className={styles.size}>{formatBytes(bytes.length)}</span>
      </div>
      <p className={styles.line}>
        <span className={styles.kind}>Text</span>
        <span className={styles.content}>{toSymbolicText(bytes)}</span>
      </p>
      {showHex && (
        <p className={styles.line} data-muted>
          <span className={styles.kind}>Hex</span>
          <span className={styles.content}>{toHex(bytes)}</span>
        </p>
      )}
    </li>
  );
}

/** One captured segment; memoized so a new batch re-renders only the rows it adds. */
export const SegmentRow = memo(SegmentRowView);
