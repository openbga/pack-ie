import { type ReactNode, useEffect, useRef } from "react";
import { type FieldValues, FormProvider, type Path, type UseFormReturn } from "react-hook-form";
import { useNavigate } from "react-router";
import type { ConfigDocument, EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { FieldSection, Form } from "@/components/Form";
import { PageHeader } from "@/components/Page";
import { StatusBadge } from "@/components/StatusBadge";
import { useToast } from "@/components/Toast";
import { type FieldPaths, toFormField } from "@/lib/errors";
import styles from "@/styles/layout.module.css";
import { ApplyReviewDialog, CheckSummary } from "./ApplyParts";
import { appliedToast } from "./applyDocument";
import { DeleteEntity, type EditedEntity } from "./DeleteEntity";
import { useApplyFlow } from "./useApplyFlow";
import { useUnsavedGuard } from "./useUnsavedGuard";

/** A document built from the form, with the identity of the record in it. */
export interface BuiltDocument {
  document: ConfigDocument;
  id: string;
}

interface ConfigEditorProps<Values extends FieldValues> {
  noun: "endpoint" | "channel";
  /** The configuration as loaded; its version is the one the change is applied on. */
  doc: ConfigDocument;
  form: UseFormReturn<Values>;
  /** The record being edited; null when creating one. */
  entity: EditedEntity | null;
  /** Where the record sits in the document's list, which is how the engine names its fields. */
  entityIndex: number;
  /** The document with the form's record in it. */
  build: (values: Values) => BuiltDocument;
  /** Fetches the configuration again, after a conflict. */
  onReload: () => Promise<unknown>;
  /** The engine's rejection of a connection test, shown on the fields it names (endpoints). */
  testRejection?: EngineError | null;
  /** The form's field sections. */
  children: ReactNode;
}

/**
 * The record page in its edit state, shared by endpoints and channels: header with the
 * "Editing" label, the form, the validate / confirm / apply flow, the unsaved-changes guard
 * and delete. The page supplies the field sections and how the form becomes a document.
 */
export function ConfigEditor<Values extends FieldValues>({
  noun,
  doc,
  form,
  entity,
  entityIndex,
  build,
  onReload,
  testRejection = null,
  children,
}: ConfigEditorProps<Values>) {
  const navigate = useNavigate();
  const toast = useToast();
  const list = `/${noun}s`;
  const savedId = useRef<string | null>(null);
  const guard = useUnsavedGuard(form.formState.isDirty);
  const flow = useApplyFlow({
    onApplied: (result, applied) => {
      toast.show(appliedToast(result, applied.channels));
      guard.allowLeave();
      void navigate(`${list}/${savedId.current ?? entity?.id ?? ""}`);
    },
  });
  const { forgetPassingCheck } = flow;
  // Synchronizes with the form library, which lives outside React: editing a value
  // invalidates a passing check.
  useEffect(() => {
    const subscription = form.watch(() => forgetPassingCheck());
    return () => subscription.unsubscribe();
  }, [form, forgetPassingCheck]);

  const fieldPaths: FieldPaths = { [`${noun}s.${entityIndex}`]: "" };
  const title = entity?.label ?? `New ${noun}`;
  const busy = flow.isChecking || flow.isApplying;

  function review(values: Values) {
    const built = build(values);
    savedId.current = built.id;
    flow.review(built.document);
  }

  async function reload() {
    flow.reset();
    await onReload();
    toast.show({
      tone: "info",
      title: "Configuration reloaded",
      description: "Your entries are kept. Check them against the new version, then apply again.",
    });
  }

  return (
    <>
      <PageHeader
        title={title}
        status={<StatusBadge tone="neutral">Editing</StatusBadge>}
        breadcrumbs={[
          { label: noun === "endpoint" ? "Endpoints" : "Channels", href: list },
          { label: title, href: entity ? `${list}/${entity.id}/edit` : `${list}/new` },
        ]}
        actions={
          entity && (
            <DeleteEntity
              noun={noun}
              document={doc}
              entity={entity}
              onDeleted={() => {
                guard.allowLeave();
                void navigate(list);
              }}
            />
          )
        }
      />
      <FormProvider {...form}>
        <Form
          aria-label={entity ? `Edit ${noun} ${entity.label}` : `New ${noun}`}
          onSubmit={(event) => void form.handleSubmit(review)(event)}
          error={flow.error ?? testRejection}
          errorTitle={
            flow.error
              ? "Not applied: the running configuration is unchanged"
              : "Not tested: the settings were rejected"
          }
          fieldPaths={fieldPaths}
          actions={
            <>
              <Button
                type="submit"
                variant="primary"
                isPending={flow.isApplying}
                isDisabled={busy || flow.hasErrors}
              >
                Save and apply
              </Button>
              <Button
                isDisabled={busy}
                onPress={() =>
                  void form.handleSubmit((values) => flow.check(build(values).document))()
                }
              >
                Validate
              </Button>
              {flow.error?.code === "conflict" && (
                <Button onPress={() => void reload()}>Reload configuration</Button>
              )}
              <Button
                isDisabled={busy}
                onPress={() => void navigate(entity ? `${list}/${entity.id}` : list)}
              >
                Cancel
              </Button>
            </>
          }
        >
          {children}
          <FieldSection title="Apply">
            <p className={styles.hint}>
              Save and apply shows what will change and asks why before anything is applied.
            </p>
            <CheckSummary
              result={flow.checked}
              onFocusPath={(path) => form.setFocus(toFormField(path, fieldPaths) as Path<Values>)}
            />
          </FieldSection>
        </Form>
      </FormProvider>
      <ApplyReviewDialog
        current={doc}
        staged={flow.staged}
        changeNote={flow.changeNote}
        isPending={flow.isApplying}
        onConfirm={flow.confirm}
        onCancel={flow.cancelReview}
      />
      {guard.dialog}
    </>
  );
}
