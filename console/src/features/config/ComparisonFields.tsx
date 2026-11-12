import { useFormContext, useWatch } from "react-hook-form";
import type { FilterOperator } from "@/api/types";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import { FormCheckbox } from "./FormCheckbox";
import { TEXT_OPERATORS, takesValue } from "./ruleForm";

/** Every operator of docs/spec/configuration.md §8.1, in the words of a sentence. */
const OPERATOR_LABELS: Record<FilterOperator, string> = {
  equals: "equals",
  not_equals: "does not equal",
  in: "is one of",
  not_in: "is not one of",
  starts_with: "starts with",
  contains: "contains",
  matches: "matches (regular expression)",
  exists: "is present",
  not_exists: "is absent",
  gt: "is greater than",
  gte: "is greater than or equal to",
  lt: "is less than",
  lte: "is less than or equal to",
};

const OPERATOR_OPTIONS = Object.entries(OPERATOR_LABELS).map(([id, label]) => ({ id, label }));

function valueHelp(operator: FilterOperator): string | undefined {
  switch (operator) {
    case "in":
    case "not_in":
      return "Separate the values with commas, for example GLU, NA, K.";
    case "matches":
      return "A regular expression of at most 200 characters.";
    case "gt":
    case "gte":
    case "lt":
    case "lte":
      return "A number; text that is not a number never matches.";
    default:
      return undefined;
  }
}

interface ComparisonFieldsProps {
  /** The form path of the comparison: `filters.0` or `mappings.2.condition`. */
  prefix: string;
  /** Paths may start `$.` inside a `for_each`. */
  inLoop?: boolean;
}

/** One comparison of a field, as filters and `when` conditions use it. */
export function ComparisonFields({ prefix, inLoop = false }: ComparisonFieldsProps) {
  const { control } = useFormContext();
  const operator = useWatch({ control, name: `${prefix}.operator` }) as FilterOperator;
  return (
    <>
      <TextInput
        name={`${prefix}.path`}
        label="Path"
        isRequired
        description={
          inLoop
            ? "For example PID.3[0].1, R.3 or $.4 for the current occurrence."
            : "For example PID.3[0].1, R.3 or col.sample_id."
        }
      />
      <ChoiceInput
        name={`${prefix}.operator`}
        label="Operator"
        options={OPERATOR_OPTIONS}
        isRequired
      />
      {takesValue(operator) && (
        <TextInput
          name={`${prefix}.value`}
          label="Value"
          isRequired
          description={valueHelp(operator)}
        />
      )}
      {TEXT_OPERATORS.has(operator) && (
        <FormCheckbox name={`${prefix}.ignoreCase`} label="Ignore case" />
      )}
    </>
  );
}
