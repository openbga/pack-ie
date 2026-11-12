import type { ReactNode } from "react";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";

interface ConfirmDialogProps {
  title: string;
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  /** What will happen, in operator terms. */
  children: ReactNode;
  /** The action's name, such as "Release report"; never just "OK". */
  confirmLabel: string;
  onConfirm: () => void;
  isPending?: boolean;
  /** `danger` for destructive or irreversible actions. */
  variant?: "primary" | "danger";
}

/**
 * Confirms an irreversible action. Focus starts on Cancel, and the dialog stays open
 * until the caller closes it, so the outcome of `onConfirm` can be shown. Escape cancels,
 * except while the action runs; an outside click does nothing. Enter in a field of
 * `children`, such as a password, confirms.
 */
export function ConfirmDialog({
  title,
  isOpen,
  onOpenChange,
  children,
  confirmLabel,
  onConfirm,
  isPending = false,
  variant = "danger",
}: ConfirmDialogProps) {
  return (
    <Dialog
      title={title}
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      role="alertdialog"
      isDismissable={false}
      isKeyboardDismissDisabled={isPending}
      onSubmit={() => {
        if (!isPending) onConfirm();
      }}
      actions={
        <>
          <Button autoFocus isDisabled={isPending} onPress={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" variant={variant} isPending={isPending}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}
