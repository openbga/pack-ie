// The text steps of a rule: an ordered list applied to the text read from `from`, before the rule
// uses it (docs/spec/configuration.md §8.2, text steps).
import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import type { StepKind } from "@/api/types";
import { FieldList } from "@/components/FieldList";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import styles from "./config.module.css";
import { emptyStep } from "./stepForm";
import { wholeNumber } from "./validators";

const KINDS: { id: StepKind; label: string }[] = [
  { id: "trim", label: "Trim spaces" },
  { id: "strip_leading", label: "Remove leading characters" },
  { id: "split", label: "Take one part of a split" },
  { id: "slice", label: "Keep part of the text" },
  { id: "replace", label: "Replace text" },
  { id: "upper", label: "Upper case" },
  { id: "lower", label: "Lower case" },
];

const KIND_HELP: Record<StepKind, string> = {
  trim: "Removes spaces and line breaks at both ends.",
  strip_leading:
    "Removes leading characters that are in the set, but never the last character: 000120 becomes 120, 000 becomes 0.",
  split:
    "Splits the text on the separator and keeps one part: 0 is the first, 1 the second, -1 the last. A part that does not exist is empty.",
  slice:
    "Keeps at most Length characters from Start (the first character is 0). Empty when Start is past the end.",
  replace: "Replaces every occurrence of the text, exactly as typed, with the other text.",
  upper: "Changes letters to upper case.",
  lower: "Changes letters to lower case.",
};

/** A text that may be a space or a comma: only its length is checked here, the engine checks the rest. */
const withinLength = (max: number) => (value: string) =>
  value.length <= max || `Use at most ${max} characters.`;

function StepFields({ name }: { name: string }) {
  const { control } = useFormContext();
  const kind = useWatch({ control, name: `${name}.kind` }) as StepKind;
  return (
    <>
      <ChoiceInput
        name={`${name}.kind`}
        label="Step"
        options={KINDS}
        isRequired
        description={KIND_HELP[kind]}
      />
      {kind === "strip_leading" && (
        <TextInput
          name={`${name}.chars`}
          label="Characters"
          description="1 to 16 characters, for example 0 or #."
          validate={withinLength(16)}
        />
      )}
      {kind === "split" && (
        <>
          <TextInput
            name={`${name}.separator`}
            label="Separator"
            description="1 to 16 characters, for example / or a space."
            validate={withinLength(16)}
          />
          <TextInput
            name={`${name}.take`}
            label="Part number"
            isRequired
            description="0 for the first part, -1 for the last."
            validate={(value) =>
              (/^-?\d+$/.test(value) && Math.abs(Number(value)) <= 100) ||
              "Enter a whole number from -100 to 100."
            }
          />
        </>
      )}
      {kind === "slice" && (
        <>
          <TextInput
            name={`${name}.start`}
            label="Start"
            isRequired
            description="The first character kept; the first character of the text is 0."
            validate={wholeNumber({ min: 0, max: 65_535 })}
          />
          <TextInput
            name={`${name}.length`}
            label="Length"
            description="At most this many characters. Leave empty to keep the rest."
            validate={wholeNumber({ min: 1, max: 65_535 })}
          />
        </>
      )}
      {kind === "replace" && (
        <>
          <TextInput
            name={`${name}.find`}
            label="Find"
            description="1 to 64 characters, matched exactly, upper and lower case apart."
            validate={withinLength(64)}
          />
          <TextInput
            name={`${name}.with`}
            label="Replace with"
            description="Leave empty to remove the text found."
            validate={withinLength(64)}
          />
        </>
      )}
    </>
  );
}

interface StepsEditorProps {
  /** The form path of the list: `destinations.0.mappings.1.steps`. */
  name: string;
  /** Names what the steps are for in labels, such as "Destination 1 mapping rule 2". */
  label: string;
}

/** The steps applied, in order, to the text a rule reads. Empty is the usual case. */
export function StepsEditor({ name, label }: StepsEditorProps) {
  const { control } = useFormContext();
  const { fields, append, remove, move } = useFieldArray({ control, name });
  const owner = label.toLowerCase();
  const stepOf = (index: number) => `Step ${index + 1} of ${owner}`;
  return (
    <div className={styles.nested}>
      <FieldList
        name={name}
        legend={`Text steps of ${owner}`}
        description="Run in order on the text read, before the rule uses it. Leave empty to use the text as it is."
        emptyText="No text steps."
        items={fields}
        itemLabel={stepOf}
        addLabel={`Add text step to ${owner}`}
        onAdd={() => append(emptyStep())}
        onMove={move}
        onRemove={remove}
        renderItem={(index) => <StepFields name={`${name}.${index}`} />}
      />
    </div>
  );
}
