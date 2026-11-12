import { useFieldArray, useFormContext } from "react-hook-form";
import { FieldList } from "@/components/FieldList";
import { ConditionFields } from "./ConditionFields";
import { nextId } from "./configDocument";
import { emptyCondition, type FilterFormValues } from "./ruleForm";

interface FilterEditorProps {
  /** The form path of the filters: `filters` or `destinations.0.filters`. */
  name: string;
  /** Names the filters in labels: `Source` or `Destination 1`. */
  owner: string;
  description: string;
}

/** Ordered filters: a message passes only when every one matches. */
export function FilterEditor({ name, owner, description }: FilterEditorProps) {
  const { control, getValues } = useFormContext();
  const { fields, append, remove, move } = useFieldArray({ control, name });
  return (
    <FieldList
      name={name}
      legend={`${owner} filter rules`}
      description={description}
      emptyText="No filters."
      items={fields}
      itemLabel={(index) => `${owner} filter ${index + 1}`}
      addLabel={`Add ${owner.toLowerCase()} filter`}
      onAdd={() => {
        const rows = (getValues(name) ?? []) as FilterFormValues[];
        append({
          ruleId: nextId(
            "f",
            rows.map((row) => row.ruleId),
          ),
          ...emptyCondition(),
        });
      }}
      onMove={move}
      onRemove={remove}
      renderItem={(index) => (
        <ConditionFields prefix={`${name}.${index}`} label={`${owner} filter ${index + 1}`} />
      )}
    />
  );
}
