import { Link } from "react-router";
import { StatusBadge } from "@/components/StatusBadge";
import styles from "./PressureMeter.module.css";

/** From here a buffer or queue is filling up; from the second, it is about to block. */
const HIGH = 0.8;
const CRITICAL = 0.95;

/**
 * How full a buffer or queue is: a labelled bar with the numbers beside it, so the level is
 * never carried by color alone. Above 80% a badge says so in words.
 */
export function PressureMeter({
  label,
  href,
  fraction,
  detail,
}: {
  /** What is measured, such as "Lab analyzer → LIS buffer". */
  label: string;
  /** Makes the label a link to the thing measured. */
  href?: string;
  /** 0 to 1. */
  fraction: number;
  /** The numbers behind it, such as "1.2 MB of 2.0 MB". */
  detail: string;
}) {
  const clamped = Math.min(1, Math.max(0, fraction));
  const percent = Math.round(clamped * 100);
  const level = clamped >= CRITICAL ? "critical" : clamped >= HIGH ? "high" : "normal";
  return (
    <div className={styles.meter}>
      <span className={styles.label}>{href ? <Link to={href}>{label}</Link> : label}</span>
      {/* biome-ignore lint/a11y/useSemanticElements: a <meter> cannot be styled the same in every web engine this app runs in. */}
      <div
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={`${percent}% full, ${detail}`}
        className={styles.track}
        data-level={level}
      >
        <div className={styles.fill} style={{ inlineSize: `${percent}%` }} />
      </div>
      <span className={styles.value}>
        <span>
          {percent}% · {detail}
        </span>
        {level === "high" && <StatusBadge tone="warning">Filling up</StatusBadge>}
        {level === "critical" && <StatusBadge tone="danger">Nearly full</StatusBadge>}
      </span>
    </div>
  );
}
