import { useCallback, useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type {
  ApplyPlan,
  ApplyResult,
  ConfigDocument,
  ConfigIssue,
  ValidationResult,
} from "@/api/types";
import { useApplyDocument, validationFailure } from "./applyDocument";
import { useChangeNote } from "./changeNote";

interface ApplyFlowOptions {
  /** Called once the engine has applied `applied` as a new version. */
  onApplied: (result: ApplyResult, applied: ConfigDocument) => void;
}

/** A validated document waiting for confirmation, with what the engine says applying does. */
export interface StagedApply {
  document: ConfigDocument;
  /** What the engine says applying does. */
  plan: ApplyPlan;
  warnings: ConfigIssue[];
}

/**
 * The validate-then-apply flow of the edit pages. `check` asks the engine to validate a
 * document without applying it. `review` validates too and, only when the document is valid,
 * stages it with the engine's plan; `confirm` then applies it. The apply validates again, and
 * an invalid document leaves the running configuration active and comes back as `error`.
 * While the last validation found errors, `hasErrors` is set: nothing can be staged then.
 */
export function useApplyFlow({ onApplied }: ApplyFlowOptions) {
  const validation = useEngineMutation((api, document: ConfigDocument) =>
    api.validateConfig(document),
  );
  const apply = useApplyDocument();
  const [checked, setChecked] = useState<ValidationResult | null>(null);
  const [staged, setStaged] = useState<StagedApply | null>(null);
  const changeNote = useChangeNote();

  function check(document: ConfigDocument) {
    apply.reset();
    setChecked(null);
    validation.mutate(document, { onSuccess: setChecked });
  }

  function review(document: ConfigDocument) {
    apply.reset();
    setChecked(null);
    validation.mutate(document, {
      onSuccess: (result) => {
        setChecked(result);
        if (!result.ok) return;
        const warnings = result.issues.filter((issue) => issue.severity === "warning");
        setStaged({ document, plan: result.plan, warnings });
      },
    });
  }

  function confirm() {
    if (staged === null || !changeNote.check()) return;
    const { document } = staged;
    validation.reset();
    setChecked(null);
    apply.mutate(
      { document, note: changeNote.note },
      {
        onSuccess: (result) => {
          setStaged(null);
          changeNote.reset();
          onApplied(result, document);
        },
        // The refusal shows on the form, not behind the dialog.
        onError: () => setStaged(null),
      },
    );
  }

  const forgetPassingCheck = useCallback(() => setChecked((last) => (last?.ok ? null : last)), []);

  function reset() {
    validation.reset();
    apply.reset();
    setChecked(null);
  }

  const problems = checked !== null && !checked.ok ? validationFailure(checked) : null;
  const hasErrors = problems !== null;
  return {
    check,
    review,
    cancelReview: () => {
      setStaged(null);
      changeNote.reset();
    },
    confirm,
    staged,
    changeNote,
    checked,
    hasErrors,
    /** A passing check no longer holds once the input is edited. */
    forgetPassingCheck,
    reset,
    error: apply.error ?? validation.error ?? problems,
    isChecking: validation.isPending,
    isApplying: apply.isPending,
  };
}
