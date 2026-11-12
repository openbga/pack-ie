import type { ReactNode } from "react";
import styles from "./ShapeChip.module.css";
import { FILLED_SHAPES, type Shape } from "./stateShapes";

/**
 * A state as a compact chip: its shape, then its words (and anything that belongs to the
 * state, such as a retry countdown).
 */
export function ShapeChip({
  shape,
  children,
}: {
  shape: Shape;
  /** Replaces the shape's label, such as "Retrying in 5 s". */
  children?: ReactNode;
}) {
  const { Icon } = shape;
  return (
    <span className={styles.chip} data-tone={shape.tone}>
      <Icon
        aria-hidden="true"
        className={styles.icon}
        data-filled={FILLED_SHAPES.has(Icon) ? "" : undefined}
      />
      {children ?? shape.label}
    </span>
  );
}
