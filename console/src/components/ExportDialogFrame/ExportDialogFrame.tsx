import type { ReactNode } from "react";
import type { EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { ErrorState } from "@/components/ErrorState";
import { Select } from "@/components/Select";
import { useToast } from "@/components/Toast";
import layout from "@/styles/layout.module.css";

interface ExportDialogFrameProps {
  title: string;
  /** One line under the title on what the file holds. */
  description: string;
  /** The submit button's label; "Export" unless the dialog names the format. */
  submitLabel?: string;
  /** The export's state: it cannot be cancelled or dismissed while it runs. */
  command: { error: EngineError | null; isPending: boolean };
  /** Heading of the engine's refusal, such as "The export failed". */
  errorTitle: string;
  /** A line under the refusal, such as what is left behind. */
  errorNote?: string;
  onSubmit: () => void;
  onClose: () => void;
  /** The fields and notes, stacked; the refusal follows them. */
  children: ReactNode;
}

/**
 * The shell of a dialog that saves a file: the stacked content, Cancel and the submit button,
 * and the refusal. Neither Escape nor Cancel closes it while the export runs. Mount it only
 * while it is open.
 */
export function ExportDialogFrame({
  title,
  description,
  submitLabel = "Export",
  command,
  errorTitle,
  errorNote,
  onSubmit,
  onClose,
  children,
}: ExportDialogFrameProps) {
  return (
    <Dialog
      title={title}
      description={description}
      isOpen
      onOpenChange={(open) => {
        if (!open && !command.isPending) onClose();
      }}
      isDismissable={!command.isPending}
      onSubmit={() => {
        if (!command.isPending) onSubmit();
      }}
      actions={
        <>
          <Button isDisabled={command.isPending} onPress={onClose}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" isPending={command.isPending}>
            {submitLabel}
          </Button>
        </>
      }
    >
      <div className={layout.stack}>
        {children}
        {command.error &&
          (errorNote ? (
            <div className={layout.stack}>
              <ErrorState title={errorTitle} error={command.error} />
              <p>{errorNote}</p>
            </div>
          ) : (
            <ErrorState title={errorTitle} error={command.error} />
          ))}
      </div>
    </Dialog>
  );
}

/**
 * The `onSuccess` of an export: closes the dialog, then announces the saved file. A null result
 * means the save dialog was cancelled, which is a choice and not an outcome to announce.
 */
export function useExportDone(onClose: () => void) {
  const toast = useToast();
  return <Saved,>(title: string, describe: (saved: Saved) => string) =>
    (saved: Saved | null) => {
      onClose();
      if (saved === null) return;
      toast.show({ tone: "success", title, description: describe(saved) });
    };
}

interface ExportFormatSelectProps<Id extends string> {
  formats: readonly { id: Id; label: string }[];
  value: Id;
  onChange: (format: Id) => void;
  isDisabled?: boolean;
}

/** The "Format" choice of an export dialog. */
export function ExportFormatSelect<Id extends string>({
  formats,
  value,
  onChange,
  isDisabled,
}: ExportFormatSelectProps<Id>) {
  return (
    <Select
      label="Format"
      name="format"
      options={formats}
      value={value}
      isDisabled={isDisabled}
      onChange={(next) => {
        const chosen = formats.find((each) => each.id === next);
        if (chosen) onChange(chosen.id);
      }}
    />
  );
}
