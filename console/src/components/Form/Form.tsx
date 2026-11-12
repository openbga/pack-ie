import {
  type FormEvent,
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Form as AriaForm } from "react-aria-components";
import type { EngineError } from "@/api/types";
import { ErrorState } from "@/components/ErrorState";
import { type FieldPaths, toUserMessage, toValidationErrors } from "@/lib/errors";
import styles from "./Form.module.css";

interface FormProps {
  children: ReactNode;
  /** Buttons shown below the fields, primary action first. */
  actions: ReactNode;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  /**
   * The engine's answer to the last submit. Its field errors show on the fields with the
   * matching `name`; its message and support reference show above the fields, followed by
   * any field error that no field on the form shows.
   */
  error?: EngineError | null;
  /** Heading of the error message, such as "Sign-in failed". */
  errorTitle?: string;
  /** Where the form's field names differ from the command input's (see `toFormField`). */
  fieldPaths?: FieldPaths;
  /** Keep long task-form actions available while scrolling within the form. */
  stickyActions?: boolean;
  "aria-label"?: string;
}

const NO_NAMES: ReadonlySet<string> = new Set();

/**
 * The error as the box above the fields words it. A field error no field shows, such as one
 * about the instrument behind the form, is the reason itself: it joins the engine's message,
 * or replaces it when no field is highlighted, since "Check the highlighted fields" would
 * then point at nothing.
 */
function withUnshownErrors(error: EngineError, unshown: readonly string[]): EngineError {
  if (unshown.length === 0) return error;
  const highlighted = error.fieldErrors.some((fieldError) => !unshown.includes(fieldError.message));
  const lead = highlighted ? [toUserMessage(error)] : [];
  return { ...error, message: [...lead, ...unshown].join(" ") };
}

/**
 * Vertical form layout. Pair with react-hook-form and the field components. When the engine
 * reports field errors, focus moves to the first invalid field, and a field's error goes away
 * as soon as its value is edited.
 */
export function Form({
  children,
  actions,
  error = null,
  errorTitle,
  fieldPaths,
  stickyActions = false,
  ...rest
}: FormProps) {
  const formRef = useRef<HTMLFormElement>(null);
  const fieldsRef = useRef<HTMLDivElement>(null);
  // Fields edited since the engine's last answer. React Aria clears an engine error only on the
  // input's change event, which a text box fires on blur, so typing a fix left it showing.
  const [edited, setEdited] = useState({ error, names: NO_NAMES });
  const editedNames = edited.error === error ? edited.names : NO_NAMES;
  const validationErrors = toValidationErrors(error, fieldPaths);
  for (const name of editedNames) delete validationErrors[name];
  // React's own input event, so this update renders together with the field's `onChange`. A
  // listener outside React rendered first and put back the old value, losing the first key.
  function onInput(event: FormEvent<HTMLDivElement>) {
    const { name } = event.target as HTMLInputElement;
    if (!name) return;
    setEdited((last) => {
      const names = last.error === error ? last.names : NO_NAMES;
      return names.has(name) ? last : { error, names: new Set(names).add(name) };
    });
  }
  // Field errors whose message no field shows. Read from the rendered fields, since only they
  // know which errors they display, before the browser paints the box above them.
  const [unshown, setUnshown] = useState({ error, messages: [] as string[] });
  useLayoutEffect(() => {
    const shown = fieldsRef.current?.textContent ?? "";
    const messages = [...new Set(error?.fieldErrors.map((fieldError) => fieldError.message))];
    setUnshown({ error, messages: messages.filter((message) => !shown.includes(message)) });
  }, [error]);
  const unshownMessages = unshown.error === error ? unshown.messages : [];
  // Moves DOM focus, which lives outside React, once the errors are on the fields.
  useEffect(() => {
    if (!error?.fieldErrors.length) return;
    formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
  }, [error]);
  return (
    <AriaForm
      {...rest}
      ref={formRef}
      validationBehavior="aria"
      validationErrors={validationErrors}
      className={styles.form}
      data-sticky-actions={stickyActions || undefined}
    >
      {error && (
        <ErrorState
          title={errorTitle ?? "The action failed"}
          error={withUnshownErrors(error, unshownMessages)}
        />
      )}
      {/* React Aria's Form drops `onInput`; this wrapper takes it without changing the layout. */}
      <div ref={fieldsRef} className={styles.fields} onInput={onInput}>
        {children}
      </div>
      <div className={styles.actions}>{actions}</div>
    </AriaForm>
  );
}
