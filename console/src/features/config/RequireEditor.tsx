// Source preconditions of a destination's mapping: checks on the message received, run before any
// rule (docs/spec/configuration.md §8.2, `require`).
import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import { FieldList } from "@/components/FieldList";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import { emptyRequire, type RequireFormValues } from "./requireForm";
import { wholeNumber } from "./validators";

const KINDS = [
  { id: "count", label: "Number of records" },
  { id: "same", label: "Same value in every record" },
];

function RequireFields({ name }: { name: string }) {
  const { control, getValues } = useFormContext();
  const kind = useWatch({ control, name: `${name}.kind` }) as RequireFormValues["kind"];
  const checkLimit = (value: string) => {
    const ok = wholeNumber({ min: 0, max: 65_535 })(value);
    return ok === true ? limitsOk() : ok;
  };
  const limitsOk = () => {
    const row = getValues(name) as RequireFormValues;
    if (row.min.trim() === "" && row.max.trim() === "") return "Set a minimum, a maximum or both.";
    if (row.min.trim() !== "" && row.max.trim() !== "" && Number(row.min) > Number(row.max)) {
      return "The minimum is more than the maximum.";
    }
    return true;
  };
  return (
    <>
      <ChoiceInput name={`${name}.kind`} label="Check" options={KINDS} isRequired />
      {kind === "count" ? (
        <>
          <TextInput
            name={`${name}.record`}
            label="Record or segment"
            isRequired
            description="For example OBR or R: the occurrences of it in the message are counted."
          />
          <TextInput
            name={`${name}.min`}
            label="Minimum"
            description="0 to 65535. Leave empty for no minimum."
            validate={checkLimit}
          />
          <TextInput
            name={`${name}.max`}
            label="Maximum"
            description="0 to 65535. Leave empty for no maximum."
            validate={checkLimit}
          />
        </>
      ) : (
        <TextInput
          name={`${name}.path`}
          label="Path"
          isRequired
          description="For example OBR.3.1: every occurrence of the record must hold the same text there. A missing value counts as empty; one occurrence always passes."
        />
      )}
    </>
  );
}

interface RequireEditorProps {
  /** The form path of the destination: `destinations.0`. */
  root: string;
  owner: string;
}

export function RequireEditor({ root, owner }: RequireEditorProps) {
  const { control } = useFormContext();
  const name = `${root}.require`;
  const { fields, append, remove, move } = useFieldArray({ control, name });
  const label = owner.toLowerCase();
  return (
    <FieldList
      name={name}
      legend={`${owner} source requirements`}
      description="Checked on the message received, before any rule runs. A message that fails one fails this destination with a transform error that names the check; nothing is filtered. For example one specimen with up to 18 orders: OBR from 1 to 18, and the same value in OBR.3.1."
      emptyText="No requirements."
      items={fields}
      itemLabel={(index) => `Requirement ${index + 1} of ${label}`}
      addLabel={`Add requirement to ${label}`}
      onAdd={() => append(emptyRequire())}
      onMove={move}
      onRemove={remove}
      renderItem={(index) => <RequireFields name={`${name}.${index}`} />}
    />
  );
}
