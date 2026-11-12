import { ChevronDown, ChevronRight, Folder, type LucideIcon } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import {
  Button,
  Dialog,
  DialogTrigger,
  Heading,
  Popover,
  Tooltip,
  TooltipTrigger,
} from "react-aria-components";
import type { QueueCount } from "./NavItem";
import frame from "./Sidebar.module.css";
import styles from "./SideNav.module.css";

/**
 * A disclosure when expanded; a labeled link popover when the sidebar is compact. A closed
 * disclosure shows its entries' total count, which the open entries show one by one.
 */
export function NavGroup({
  heading,
  icon: Icon = Folder,
  isOpen,
  onToggle,
  compact,
  active,
  count,
  children,
}: {
  heading: string;
  icon: LucideIcon | undefined;
  isOpen: boolean;
  onToggle: () => void;
  compact: boolean;
  active: boolean;
  count: QueueCount | null;
  children: (onNavigate?: () => void) => ReactNode;
}) {
  const id = useId();
  const [flyoutOpen, setFlyoutOpen] = useState(false);
  const Chevron = isOpen ? ChevronDown : ChevronRight;
  const label = count ? `${heading}, ${count.spoken}` : heading;
  if (compact) {
    return (
      <li>
        <DialogTrigger isOpen={flyoutOpen} onOpenChange={setFlyoutOpen}>
          <TooltipTrigger>
            <Button
              aria-label={label}
              aria-current={active ? "true" : undefined}
              className={styles.railButton}
              data-current={active || undefined}
            >
              <Icon aria-hidden="true" className={styles.icon} />
              {count && <span aria-hidden="true" className={styles.dot} />}
            </Button>
            <Tooltip placement="right" className={frame.tooltip}>
              {label}
            </Tooltip>
          </TooltipTrigger>
          <Popover placement="right top" className={styles.popover}>
            <Dialog aria-label={heading} className={styles.flyout}>
              <Heading slot="title" className={styles.flyoutTitle}>
                {heading}
              </Heading>
              <nav aria-label={heading}>
                <ul className={styles.list}>{children(() => setFlyoutOpen(false))}</ul>
              </nav>
            </Dialog>
          </Popover>
        </DialogTrigger>
      </li>
    );
  }
  return (
    <li className={styles.section}>
      <button
        type="button"
        id={`${id}-heading`}
        aria-label={isOpen ? undefined : label}
        aria-expanded={isOpen}
        aria-controls={isOpen ? `${id}-list` : undefined}
        className={styles.group}
        data-current={active || undefined}
        onClick={onToggle}
      >
        <Icon aria-hidden="true" className={styles.icon} />
        <span className={styles.label}>{heading}</span>
        {!isOpen && count && (
          <span aria-hidden="true" className={styles.count}>
            {count.shown}
          </span>
        )}
        <Chevron aria-hidden="true" className={styles.chevron} />
      </button>
      {isOpen && (
        <ul id={`${id}-list`} aria-labelledby={`${id}-heading`} className={styles.groupList}>
          {children()}
        </ul>
      )}
    </li>
  );
}
