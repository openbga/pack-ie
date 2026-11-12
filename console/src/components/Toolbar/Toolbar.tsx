import { type ReactNode, useId, useRef, useState } from "react";
import { Group } from "react-aria-components";
import { Button } from "@/components/Button";
import styles from "./Toolbar.module.css";

interface ToolbarProps {
  /** Buttons and controls, in reading order. */
  children?: ReactNode;
  /** Collection search, filters and actions with a trailing count. */
  variant?: "collection";
  /** One canonical count; announced without moving focus. Omit until known. */
  count?: ReactNode;
  /** Refresh and other list actions follow common and advanced filter controls. */
  actions?: ReactNode;
  /** Less-used filters; applied values remain visible while the panel is closed. */
  moreFilters?: { content: ReactNode; applied?: string | undefined };
  /** Align labeled fields with adjacent buttons at their control edge. */
  align?: "center" | "end";
  /**
   * Counts and limits only, such as "3 selected" or "Shows the newest 500". Never a usage
   * hint such as "Select a row to …": an interaction that needs one is hidden.
   */
  hint?: string;
}

/** A row of actions that wraps on narrow windows, with an optional count or limit after them. */
export function Toolbar({
  children,
  hint,
  align = "center",
  variant,
  count,
  actions,
  moreFilters,
}: ToolbarProps) {
  const [expanded, setExpanded] = useState(false);
  const panelId = useId();
  const toggle = useRef<HTMLButtonElement>(null);
  if (!children && !actions && !hint && !moreFilters && (count == null || count === false)) {
    return null;
  }
  return (
    <div className={styles.group} data-variant={variant}>
      <div className={styles.toolbar} data-align={align}>
        {children}
        {moreFilters && (
          <Button
            ref={toggle}
            variant="quiet"
            aria-expanded={expanded}
            aria-controls={panelId}
            onPress={() => setExpanded(!expanded)}
          >
            More filters
          </Button>
        )}
        {actions}
        {hint && <span className={styles.hint}>{hint}</span>}
        {count != null && count !== false && (
          <span className={styles.count} role="status" aria-atomic="true">
            {count}
          </span>
        )}
      </div>
      {moreFilters && (
        <>
          {moreFilters.applied && <p className={styles.hint}>{moreFilters.applied}</p>}
          <Group
            aria-label="More filters"
            id={panelId}
            hidden={!expanded}
            className={styles.filters}
            onKeyDown={(event) => {
              if (event.key !== "Escape" || event.defaultPrevented) return;
              event.stopPropagation();
              setExpanded(false);
              toggle.current?.focus();
            }}
          >
            {moreFilters.content}
          </Group>
        </>
      )}
    </div>
  );
}
