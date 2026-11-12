import { type ReactNode, useId } from "react";
import styles from "./Section.module.css";

interface SectionProps {
  /** The section's `h2`. */
  title: ReactNode;
  /** Buttons beside the heading, such as a decision on this part of the record. */
  actions?: ReactNode;
  /** Plain sections group fields within a task without nesting another bordered surface. */
  variant?: "surface" | "plain";
  children: ReactNode;
}

/** One titled part of a screen, named by its heading for assistive technology. */
export function Section({ title, actions, children, variant = "surface" }: SectionProps) {
  const id = useId();
  const heading = (
    <h2 id={id} className={styles.title}>
      {title}
    </h2>
  );
  return (
    <section aria-labelledby={id} className={styles.section} data-variant={variant}>
      {actions ? (
        <div className={styles.header}>
          {heading}
          {actions}
        </div>
      ) : (
        heading
      )}
      {children}
    </section>
  );
}
