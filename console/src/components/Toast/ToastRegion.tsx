import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { toneIcons } from "@/components/tones";
import styles from "./Toast.module.css";

/** Information and success notices leave after this time, unless paused. */
const TOAST_DURATION_MS = 8000;

export interface ToastMessage {
  id: number;
  tone: "info" | "success" | "warning" | "danger";
  title: string;
  description?: string;
}

interface ToastRegionProps {
  toasts: readonly ToastMessage[];
  onDismiss: (id: number) => void;
}

/** Always rendered, so screen readers pick up each notice added to the live region. */
export function ToastRegion({ toasts, onDismiss }: ToastRegionProps) {
  return (
    <section aria-label="Notifications" className={styles.region}>
      <ol aria-live="polite" className={styles.list}>
        {toasts.map((toast) => (
          <Toast key={toast.id} toast={toast} onDismiss={onDismiss} />
        ))}
      </ol>
    </section>
  );
}

/**
 * One notice. Warnings and errors stay until dismissed, since they ask for attention
 * (WCAG 2.2 SC 2.2.1). Information and success leave by themselves, but never while the
 * pointer is over the notice or focus is in it.
 */
function Toast({ toast, onDismiss }: { toast: ToastMessage; onDismiss: (id: number) => void }) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const remaining = useRef(TOAST_DURATION_MS);
  const leaves = toast.tone === "info" || toast.tone === "success";
  const paused = hovered || focused;

  // The timer lives outside React; pausing keeps the time left for when it resumes.
  useEffect(() => {
    if (!leaves || paused) return;
    const started = Date.now();
    const timer = setTimeout(() => onDismiss(toast.id), remaining.current);
    return () => {
      clearTimeout(timer);
      remaining.current -= Date.now() - started;
    };
  }, [leaves, paused, onDismiss, toast.id]);

  const Icon = toneIcons[toast.tone];
  return (
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: hover and focus only pause the timer; Dismiss is the control.
    <li
      className={styles.toast}
      data-tone={toast.tone}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false);
      }}
    >
      <Icon aria-hidden="true" className={styles.icon} />
      <div className={styles.text}>
        <p className={styles.title}>{toast.title}</p>
        {toast.description && <p>{toast.description}</p>}
      </div>
      <Button variant="quiet" aria-label="Dismiss" onPress={() => onDismiss(toast.id)}>
        <X aria-hidden="true" />
      </Button>
    </li>
  );
}
