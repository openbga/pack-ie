import { Hourglass } from "lucide-react";
import styles from "./LoadingState.module.css";

interface LoadingStateProps {
  label?: string;
  /**
   * Placeholder rows where a grid or list will appear, so the page does not jump when it
   * arrives. Static, with no shimmer: motion stays under 150ms.
   */
  rows?: number;
}

/** Shown while the engine answers. No spinner: motion stays under 150ms. */
export function LoadingState({ label = "Loading…", rows }: LoadingStateProps) {
  return (
    <div role="status" className={styles.loading}>
      <span className={styles.label}>
        <Hourglass aria-hidden="true" className={styles.icon} />
        {label}
      </span>
      {rows !== undefined && rows > 0 && (
        <div
          aria-hidden="true"
          data-testid="skeleton"
          className={styles.skeleton}
          style={{ blockSize: `calc(${rows} * var(--control-height))` }}
        />
      )}
    </div>
  );
}
