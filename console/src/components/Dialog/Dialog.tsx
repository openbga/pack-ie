import type { FormEvent, ReactNode } from "react";
import { Dialog as AriaDialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import styles from "./Dialog.module.css";
import { useDropStaleAnnouncements } from "./staleAnnouncements";

interface DialogProps {
  title: string;
  /** One line under the title on what the dialog is for. */
  description?: string;
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  children: ReactNode;
  /** Buttons in the footer; omit when the content is a `Form` with its own actions. */
  actions?: ReactNode;
  /** `alertdialog` for confirmations that interrupt the user. */
  role?: "dialog" | "alertdialog";
  /** Closes on outside clicks, and on Escape unless `isKeyboardDismissDisabled`. */
  isDismissable?: boolean;
  /** Escape does not close the dialog. Defaults to the opposite of `isDismissable`. */
  isKeyboardDismissDisabled?: boolean;
  /**
   * Makes the content and the actions one form: Enter in a field, or a `submit` button among
   * the actions, calls it.
   */
  onSubmit?: () => void;
  /**
   * `wide` for long forms whose fields sit side by side in sections (`FieldSection`). The title
   * stays at the top and the actions at the bottom while the content scrolls between them.
   * `extra-wide` is the same, wider, for a section whose choices must fit on one line.
   */
  size?: DialogSize;
}

export type DialogSize = "default" | "wide" | "extra-wide";

/** A modal dialog: traps focus while open and returns it to the trigger when closed. */
export function Dialog({
  title,
  description,
  isOpen,
  onOpenChange,
  children,
  actions,
  role = "dialog",
  isDismissable = true,
  isKeyboardDismissDisabled = !isDismissable,
  onSubmit,
  size = "default",
}: DialogProps) {
  const content = (
    <>
      <div className={styles.body}>{children}</div>
      {actions && <div className={styles.actions}>{actions}</div>}
    </>
  );
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    // The dialog renders in a portal; keep its submit from reaching a form around the trigger.
    event.stopPropagation();
    onSubmit?.();
  }
  return (
    <ModalOverlay
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      isDismissable={isDismissable}
      isKeyboardDismissDisabled={isKeyboardDismissDisabled}
      className={styles.overlay}
    >
      <Modal
        className={styles.modal}
        data-size={size === "extra-wide" ? "wide" : size}
        data-extra-wide={size === "extra-wide" || undefined}
      >
        <AriaDialog role={role} className={styles.dialog}>
          <DropStaleAnnouncements />
          <div className={styles.header}>
            <Heading slot="title" className={styles.title}>
              {title}
            </Heading>
            {description && <p className={styles.description}>{description}</p>}
          </div>
          {onSubmit ? (
            <form onSubmit={submit} className={styles.form}>
              {content}
            </form>
          ) : (
            content
          )}
        </AriaDialog>
      </Modal>
    </ModalOverlay>
  );
}

/** Renders nothing; its cleanup runs when the dialog's content leaves the page. */
function DropStaleAnnouncements() {
  useDropStaleAnnouncements();
  return null;
}
