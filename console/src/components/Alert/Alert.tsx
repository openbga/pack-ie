import { X } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/Button";
import { toneIcons } from "@/components/tones";
import styles from "./Alert.module.css";

interface AlertProps {
  /** `hold` is for something the operator must resolve before work can go on. */
  tone: "info" | "success" | "warning" | "danger" | "hold";
  title?: string;
  /**
   * Smaller text and padding, for notes that explain a form, such as the information at the top
   * of a dialog. A `ul` inside lists several short notes.
   */
  compact?: boolean;
  children: ReactNode;
  /** Only supplied when the caller allows the person to hide this notice. */
  onDismiss?: () => void;
}

/**
 * An inline message: the tone's tint, a solid left edge, its icon and an optional title. The
 * icon lines up with the title, or is centred beside an untitled message.
 * Warnings, holds and errors are announced at once; others politely. An Alert never
 * dismisses itself; callers may explicitly offer a dismiss button.
 */
export function Alert({ tone, title, compact, children, onDismiss }: AlertProps) {
  const Icon = toneIcons[tone];
  const urgent = tone === "danger" || tone === "warning" || tone === "hold";
  return (
    <div
      role={urgent ? "alert" : "status"}
      className={styles.alert}
      data-tone={tone}
      data-titled={title ? "" : undefined}
      data-compact={compact ? "" : undefined}
    >
      <Icon aria-hidden="true" className={styles.icon} />
      <div className={styles.content}>
        {title && <p className={styles.title}>{title}</p>}
        <div>{children}</div>
      </div>
      {onDismiss && (
        <div className={styles.dismiss}>
          <Button variant="quiet" aria-label="Dismiss notification" onPress={onDismiss}>
            <X aria-hidden="true" />
          </Button>
        </div>
      )}
    </div>
  );
}
