import type { ReactNode, Ref } from "react";
import { Button as AriaButton, Link } from "react-aria-components";
import styles from "./Button.module.css";

interface ButtonProps {
  children: ReactNode;
  /** `primary` for the main action of a view, `danger` for destructive actions. */
  variant?: "primary" | "secondary" | "danger" | "quiet";
  type?: "button" | "submit";
  /**
   * Makes the control a link to another screen that looks like a button: navigation is a link
   * (it can be copied and is read as a link), an action is a button.
   */
  href?: string;
  isDisabled?: boolean;
  /** Shows the action as running and blocks repeated presses. */
  isPending?: boolean;
  autoFocus?: boolean;
  onPress?: () => void;
  "aria-label"?: string;
  "aria-expanded"?: boolean;
  "aria-controls"?: string;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  variant = "secondary",
  type = "button",
  href,
  children,
  ...rest
}: ButtonProps) {
  if (href !== undefined) {
    return (
      <Link
        href={href}
        aria-label={rest["aria-label"]}
        isDisabled={rest.isDisabled}
        autoFocus={rest.autoFocus}
        className={`${styles.button} ${styles.link}`}
        data-variant={variant}
      >
        {children}
      </Link>
    );
  }
  return (
    <AriaButton {...rest} type={type} className={styles.button} data-variant={variant}>
      {children}
    </AriaButton>
  );
}
