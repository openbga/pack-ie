import { ChevronDown, ChevronUp, X } from "lucide-react";
import { type ReactNode, useId } from "react";
import { Button } from "@/components/Button";
import { useOnIdle } from "@/lib/idle";
import styles from "./SidePanel.module.css";

interface SidePanelProps {
  title: string;
  /** Shown after the title, such as a status badge. */
  badge?: ReactNode;
  onClose: () => void;
  /** Previous and Next, for a record of a list; absent for a panel that is not in one. */
  navigation?: {
    previousLabel: string;
    nextLabel: string;
    /** Absent at the top of the list. */
    onPrevious: (() => void) | undefined;
    /** Absent at the end of the list. */
    onNext: (() => void) | undefined;
  };
  /** Closes when the console goes idle, for a panel that can show operator text. */
  closesWhenIdle?: boolean;
  size?: "narrow";
  children: ReactNode;
}

/**
 * A record beside its list: a named region with a title, Previous and Next, and a Close button.
 * Nonmodal, so the list stays usable and keeps focus; Escape closes it from inside.
 */
export function SidePanel({
  title,
  badge,
  onClose,
  navigation,
  closesWhenIdle = false,
  size,
  children,
}: SidePanelProps) {
  const headingId = useId();
  useOnIdle(() => {
    if (closesWhenIdle) onClose();
  });
  return (
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: Escape from any control inside closes the panel.
    <section
      aria-labelledby={headingId}
      className={styles.panel}
      data-size={size}
      onKeyDown={(keyEvent) => {
        if (keyEvent.key !== "Escape" || keyEvent.defaultPrevented) return;
        keyEvent.preventDefault();
        onClose();
      }}
    >
      <header className={styles.header}>
        <h2 id={headingId} className={styles.title}>
          {title}
        </h2>
        {badge}
        <div className={styles.tools}>
          {navigation && (
            <>
              <Button
                variant="quiet"
                aria-label={navigation.previousLabel}
                isDisabled={!navigation.onPrevious}
                onPress={navigation.onPrevious}
              >
                <ChevronUp aria-hidden="true" />
              </Button>
              <Button
                variant="quiet"
                aria-label={navigation.nextLabel}
                isDisabled={!navigation.onNext}
                onPress={navigation.onNext}
              >
                <ChevronDown aria-hidden="true" />
              </Button>
            </>
          )}
          <Button variant="quiet" aria-label="Close" onPress={onClose}>
            <X aria-hidden="true" />
          </Button>
        </div>
      </header>
      <div className={styles.body}>{children}</div>
    </section>
  );
}
