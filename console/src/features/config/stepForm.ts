// The form's values for text steps and for the named operands of a `calculate` formula, and how
// they become the document's types. Numbers are held as text while typed.
import type { Formula, Step, StepKind } from "@/api/types";
import {
  type ConditionFormValues,
  conditionToValues,
  emptyCondition,
  valuesToCondition,
} from "./conditionForm";

/** Every field of every step; `kind` decides which are used, the others are dropped on save. */
export interface StepFormValues {
  kind: StepKind;
  chars: string;
  separator: string;
  take: string;
  start: string;
  length: string;
  find: string;
  with: string;
}

export function emptyStep(kind: StepKind = "trim"): StepFormValues {
  return { kind, chars: "", separator: "", take: "0", start: "0", length: "", find: "", with: "" };
}

export function stepToValues(step: Step): StepFormValues {
  const base = emptyStep(step.step);
  switch (step.step) {
    case "strip_leading":
      return { ...base, chars: step.chars };
    case "split":
      return { ...base, separator: step.separator, take: String(step.take) };
    case "slice":
      return {
        ...base,
        start: String(step.start),
        length: step.length === null ? "" : String(step.length),
      };
    case "replace":
      return { ...base, find: step.find, with: step.with };
    default:
      return base;
  }
}

export function valuesToStep(values: StepFormValues): Step {
  switch (values.kind) {
    case "strip_leading":
      return { step: "strip_leading", chars: values.chars };
    case "split":
      return { step: "split", separator: values.separator, take: Number(values.take.trim()) };
    case "slice": {
      const length = values.length.trim();
      return {
        step: "slice",
        start: Number(values.start.trim()),
        length: length === "" ? null : Number(length),
      };
    }
    case "replace":
      return { step: "replace", find: values.find, with: values.with };
    default:
      return { step: values.kind };
  }
}

/** A formula operand that is more than a path: its own fields, defined beside the formula. */
export interface OperandFormValues {
  name: string;
  /** `path`: the value at a path after its steps. `find`: the same in the first matching record. */
  kind: "path" | "find";
  from: string;
  /** `find` only: the record or segment to search, and the condition it must meet. */
  find: string;
  where: ConditionFormValues;
  steps: StepFormValues[];
}

export function emptyOperand(name = ""): OperandFormValues {
  return { name, kind: "path", from: "", find: "", where: emptyCondition(), steps: [] };
}

export function operandToValues(name: string, operand: Formula): OperandFormValues {
  if ("find" in operand) {
    return {
      name,
      kind: "find",
      from: operand.from,
      find: operand.find,
      where: conditionToValues(operand.where),
      steps: operand.steps.map(stepToValues),
    };
  }
  if ("from" in operand) {
    return { ...emptyOperand(name), from: operand.from, steps: operand.steps.map(stepToValues) };
  }
  return emptyOperand(name);
}

export function valuesToOperand(values: OperandFormValues): Formula {
  const steps = values.steps.map(valuesToStep);
  if (values.kind === "find") {
    return {
      find: values.find.trim(),
      where: valuesToCondition(values.where),
      from: values.from.trim(),
      steps,
    };
  }
  return { from: values.from.trim(), steps };
}
