// One condition of a filter or a `when` rule: a comparison of a field, or a combination of
// conditions (all of them, any of them, not one) that nests as deep as the rule needs
// (docs/spec/configuration.md §8.1).
import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import { FieldList } from "@/components/FieldList";
import { ChoiceInput } from "@/components/FormFields";
import { Inset } from "@/components/Inset";
import { ComparisonFields } from "./ComparisonFields";
import styles from "./config.module.css";
import { type ConditionFormValues, type ConditionType, emptyCondition } from "./ruleForm";

const TYPES: { id: ConditionType; label: string }[] = [
  { id: "compare", label: "Compare a field" },
  { id: "all", label: "All of these hold" },
  { id: "any", label: "Any of these holds" },
  { id: "not", label: "This does not hold" },
];

const COMBINATION_HELP: Record<ConditionType, string> = {
  compare: "Compares the text at a path with a value.",
  all: "Holds only when every condition below holds.",
  any: "Holds when at least one condition below holds.",
  not: "Holds when the condition below does not hold.",
};

const comparisonOf = (values: ConditionFormValues): ConditionFormValues => ({
  ...emptyCondition(),
  path: values.path,
  operator: values.operator,
  value: values.value,
  ignoreCase: values.ignoreCase,
});

/**
 * The items a condition gets when its type changes, so that choosing another type keeps what
 * was typed: a comparison becomes the first item of a combination, a combination negated is
 * wrapped whole, and the others keep their items.
 */
function itemsAfterRetype(
  values: ConditionFormValues,
  from: ConditionType,
  to: ConditionType,
): ConditionFormValues[] {
  if (to === "compare" || from === to) return values.items;
  if (from === "compare") return [comparisonOf(values)];
  if (to === "not" && values.items.length !== 1) {
    return [{ ...emptyCondition(), type: from, items: values.items }];
  }
  return values.items;
}

interface ConditionProps {
  /** The form path of the condition: `filters.0`, `destinations.0.mappings.1.condition`. */
  prefix: string;
  /** Names the condition in the labels of what it holds: `Source filter 1`. */
  label: string;
  /** Paths may start `$.` inside a `for_each`. */
  inLoop?: boolean;
}

function ConditionItems({ prefix, label, inLoop }: ConditionProps) {
  const { control } = useFormContext();
  const name = `${prefix}.items`;
  const type = useWatch({ control, name: `${prefix}.type` }) as ConditionType;
  const { fields, append, remove, move } = useFieldArray({ control, name });
  const owner = label.toLowerCase();
  const itemLabel = (index: number) => `Condition ${index + 1} of ${owner}`;
  return (
    <FieldList
      name={name}
      legend={`${type === "all" ? "All of" : "Any of"} ${owner}`}
      description={COMBINATION_HELP[type]}
      emptyText="No conditions yet. Add at least one."
      items={fields}
      itemLabel={itemLabel}
      addLabel={`Add condition to ${owner}`}
      onAdd={() => append(emptyCondition())}
      onMove={move}
      onRemove={remove}
      renderItem={(index) => (
        <ConditionFields prefix={`${name}.${index}`} label={itemLabel(index)} inLoop={inLoop} />
      )}
    />
  );
}

/** A comparison, or a combination with the conditions it holds. */
export function ConditionFields({ prefix, label, inLoop = false }: ConditionProps) {
  const { control, getValues, setValue } = useFormContext();
  const type = useWatch({ control, name: `${prefix}.type` }) as ConditionType;
  return (
    <>
      <ChoiceInput
        name={`${prefix}.type`}
        label="Condition"
        options={TYPES}
        isRequired
        description={COMBINATION_HELP[type]}
        onChange={(chosen) => {
          const next = chosen as ConditionType;
          const items = itemsAfterRetype(getValues(prefix) as ConditionFormValues, type, next);
          setValue(`${prefix}.items`, items, { shouldDirty: true });
        }}
      />
      {type === "compare" && <ComparisonFields prefix={prefix} inLoop={inLoop} />}
      {(type === "all" || type === "any") && (
        <div className={styles.nested}>
          <ConditionItems prefix={prefix} label={label} inLoop={inLoop} />
        </div>
      )}
      {type === "not" && (
        <div className={styles.nested}>
          <Inset as="fieldset">
            <legend className={styles.legend}>Not {label.toLowerCase()}</legend>
            <ConditionFields
              prefix={`${prefix}.items.0`}
              label={`Negated condition of ${label.toLowerCase()}`}
              inLoop={inLoop}
            />
          </Inset>
        </div>
      )}
    </>
  );
}
