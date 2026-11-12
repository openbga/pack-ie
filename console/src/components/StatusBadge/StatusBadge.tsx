import { type Tone, toneIcons } from "@/components/tones";
import styles from "./StatusBadge.module.css";

interface StatusBadgeProps {
  tone: Tone;
  /** The status in words; required, because color alone never carries status. */
  children: string;
}

/** A compact status label. Callers map their states to a tone; this only displays it. */
export function StatusBadge({ tone, children }: StatusBadgeProps) {
  const Icon = toneIcons[tone];
  return (
    <span className={styles.badge} data-tone={tone}>
      <Icon aria-hidden="true" className={styles.icon} />
      {children}
    </span>
  );
}
