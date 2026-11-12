import { Link, Tooltip, TooltipTrigger } from "react-aria-components";
import type { Screen } from "./navigation";
import frame from "./Sidebar.module.css";
import styles from "./SideNav.module.css";

export interface QueueCount {
  shown: string;
  spoken: string;
}

/** The same ordinary navigation link in the full sidebar and a compact area's popover. */
export function NavItem({
  screen: { path, label, icon: Icon },
  current,
  count,
  compact,
  onNavigate,
}: {
  screen: Screen;
  current: "page" | "true" | undefined;
  count: QueueCount | null;
  compact: boolean;
  onNavigate?: () => void;
}) {
  // The visible text "Alerts 12" must be the start of the name (WCAG 2.5.3): no comma in the
  // name, and a space between the two spans so the visible text reads "Alerts 12", not "Alerts12".
  const accessibleName = count ? `${label} ${count.spoken}` : label;
  return (
    <li>
      <TooltipTrigger isDisabled={!compact}>
        <Link
          href={path}
          aria-current={current}
          aria-label={accessibleName}
          className={styles.link}
          data-compact={compact || undefined}
          onPress={onNavigate}
        >
          <Icon aria-hidden="true" className={styles.icon} />
          {!compact && <span className={styles.label}>{label}</span>}
          {count && !compact && " "}
          {count && (
            <span aria-hidden="true" className={compact ? styles.dot : styles.count}>
              {!compact && count.shown}
            </span>
          )}
        </Link>
        <Tooltip placement="right" className={frame.tooltip}>
          {accessibleName}
        </Tooltip>
      </TooltipTrigger>
    </li>
  );
}
