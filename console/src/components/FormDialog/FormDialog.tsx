import type { FormEvent, ReactNode } from "react";
import { type FieldValues, FormProvider, type UseFormReturn } from "react-hook-form";
import type { EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { Dialog, type DialogSize } from "@/components/Dialog";
import { Form } from "@/components/Form";
import type { FieldPaths } from "@/lib/errors";
import styles from "./FormDialog.module.css";

interface FormDialogProps<Values extends FieldValues> {
  title: string;
  /** One line under the title on what the dialog is for. */
  description?: string;
  submitLabel: string;
  form: UseFormReturn<Values>;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  /** The command's state: its last error and whether it is running. */
  command: { error: EngineError | null; isPending: boolean };
  /** Heading of the engine's refusal, such as "The change was not saved". */
  errorTitle: string;
  onClose: () => void;
  /** Buttons after the submit button, such as a reload offered after a conflict. */
  extraActions?: ReactNode;
  /** Where engine field errors land when the input's shape differs from the form's. */
  fieldPaths?: FieldPaths;
  /**
   * `wide` for long forms grouped in `FieldSection`s; Save and Cancel stay in view.
   * `extra-wide` likewise, for a section whose choices must fit on one line.
   */
  size?: DialogSize;
  /**
   * A note at the start of the action bar, such as what saving does not do yet. The buttons
   * then follow it at the end of the bar: Cancel, the extra actions, the submit button last.
   */
  footnote?: string;
  children: ReactNode;
}

/**
 * A dialog holding one react-hook-form form that runs one engine command. The fields reach
 * the form through `FormProvider`. Escape and outside clicks close it only while nothing is
 * typed and no command runs, so neither typed input nor the command's outcome is lost. Mount
 * it only while it is open.
 */
export function FormDialog<Values extends FieldValues>({
  title,
  description,
  submitLabel,
  form,
  onSubmit,
  command,
  errorTitle,
  onClose,
  extraActions,
  fieldPaths,
  size,
  footnote,
  children,
}: FormDialogProps<Values>) {
  const submit = (
    <Button type="submit" variant="primary" isPending={command.isPending}>
      {submitLabel}
    </Button>
  );
  const cancel = (
    <Button isDisabled={command.isPending} onPress={onClose}>
      Cancel
    </Button>
  );
  return (
    <Dialog
      title={title}
      description={description}
      isOpen
      onOpenChange={(isOpen) => {
        if (!isOpen) onClose();
      }}
      isDismissable={!command.isPending && !form.formState.isDirty}
      size={size}
    >
      <FormProvider {...form}>
        <Form
          aria-label={title}
          onSubmit={onSubmit}
          error={command.error}
          errorTitle={errorTitle}
          fieldPaths={fieldPaths}
          stickyActions={size === "wide" || size === "extra-wide"}
          actions={
            footnote ? (
              <>
                <p className={styles.footnote}>{footnote}</p>
                {cancel}
                {extraActions}
                {submit}
              </>
            ) : (
              <>
                {submit}
                {extraActions}
                {cancel}
              </>
            )
          }
        >
          {children}
        </Form>
      </FormProvider>
    </Dialog>
  );
}
