// The fields of the mapping ops that hold no rules of their own (docs/spec/configuration.md
// §8.2); `when` and `for_each` are in MappingRules.tsx, since they contain rule lists.
import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import type { LookupOnMissing, MappingOp } from "@/api/types";
import { FieldList } from "@/components/FieldList";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import styles from "./config.module.css";
import { FormCheckbox } from "./FormCheckbox";
import { PathField } from "./PathField";
import type { CodeTableFormValues, PartFormValues, RuleContext } from "./ruleForm";
import { StepsEditor } from "./StepFields";
import { wholeNumber } from "./validators";

export interface OpProps {
  /** The form path of the rule: `destinations.0.mappings.1` or `…mappings.1.rules.0`. */
  name: string;
  /** The rule's name, such as "Mapping rule 2". */
  label: string;
  /** Inside a `for_each` or an `append`, where paths may start `$.`. */
  context: RuleContext;
  /** The destination holding the rule; its `codeTables` are what a lookup chooses from. */
  root: string;
}

export function CopyFields({ name, label, context }: OpProps) {
  return (
    <>
      <PathField name={`${name}.from`} label="From" context={context} direction="from" />
      <StepsEditor name={`${name}.steps`} label={label} />
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the text is written."
      />
      <TextInput
        name={`${name}.default`}
        label="Default"
        description="Written when the source is empty or missing."
      />
      <FormCheckbox name={`${name}.required`} label="Required" />
    </>
  );
}

export function ConstantFields({ name, context }: OpProps) {
  return (
    <>
      <TextInput
        name={`${name}.value`}
        label="Value"
        description="The text to write, exactly as typed."
      />
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the text is written."
      />
    </>
  );
}

const ON_MISSING: { id: LookupOnMissing; label: string }[] = [
  { id: "error", label: "Fail the message" },
  { id: "keep", label: "Keep the code unchanged" },
  { id: "default", label: "Use the default" },
  { id: "skip", label: "Write nothing" },
];

/** The tables to choose from; a name that no table has any more stays visible. */
function useTableOptions(root: string, current: string | null) {
  const { control } = useFormContext();
  const tables = useWatch({ control, name: `${root}.codeTables` }) as CodeTableFormValues[];
  const names = [...new Set(tables.map((table) => table.name.trim()))].filter(
    (name) => name !== "",
  );
  const options = names.map((name) => ({ id: name, label: name }));
  if (current && !names.includes(current)) {
    options.push({ id: current, label: `${current} (no such table)` });
  }
  return options;
}

export function LookupFields({ name, label, context, root }: OpProps) {
  const { control } = useFormContext();
  const table = useWatch({ control, name: `${name}.table` }) as string;
  const onMissing = useWatch({ control, name: `${name}.onMissing` }) as LookupOnMissing;
  const options = useTableOptions(root, table);
  return (
    <>
      <PathField name={`${name}.from`} label="From" context={context} direction="from" />
      <StepsEditor name={`${name}.steps`} label={label} />
      <ChoiceInput
        name={`${name}.table`}
        label="Table"
        options={options}
        isRequired
        description={
          options.length === 0 ? "Add a code table to this destination below first." : undefined
        }
      />
      <FormCheckbox
        name={`${name}.ignoreCase`}
        label="Ignore case"
        description="Match the code whatever its letter case, so m, M and F, f are found alike."
      />
      <ChoiceInput name={`${name}.onMissing`} label="On missing" options={ON_MISSING} isRequired />
      {onMissing === "default" && (
        <TextInput
          name={`${name}.default`}
          label="Default"
          isRequired
          description="Written when the code is not in the table."
        />
      )}
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the result is written."
      />
    </>
  );
}

const PART_KINDS: { id: PartFormValues["kind"]; label: string }[] = [
  { id: "from", label: "From path" },
  { id: "text", label: "Text" },
];

function PartFields({
  name,
  context,
  label,
}: {
  name: string;
  context: RuleContext;
  label: string;
}) {
  const { control } = useFormContext();
  const kind = useWatch({ control, name: `${name}.kind` }) as PartFormValues["kind"];
  return (
    <>
      <ChoiceInput name={`${name}.kind`} label="Part type" options={PART_KINDS} isRequired />
      {kind === "from" ? (
        <>
          <PathField name={`${name}.from`} label="From path" context={context} direction="from" />
          <StepsEditor name={`${name}.steps`} label={label} />
        </>
      ) : (
        <TextInput
          name={`${name}.text`}
          label="Text"
          description="Written as typed; spaces count."
        />
      )}
    </>
  );
}

export function ConcatFields({ name, label, context }: OpProps) {
  const { control } = useFormContext();
  const parts = `${name}.parts`;
  const { fields, append, remove, move } = useFieldArray({ control, name: parts });
  const partOf = (index: number) => `Part ${index + 1} of ${label.toLowerCase()}`;
  const blank: PartFormValues = { kind: "from", from: "", text: "", steps: [] };
  return (
    <>
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the joined text is written."
      />
      <div className={styles.nested}>
        <FieldList
          name={parts}
          legend={`Parts of ${label.toLowerCase()}`}
          description="Joined in this order. Each part is the text at a path, or literal text."
          emptyText="No parts yet."
          items={fields}
          itemLabel={partOf}
          addLabel={`Add part to ${label.toLowerCase()}`}
          onAdd={() => append(blank)}
          onMove={move}
          onRemove={remove}
          renderItem={(index) => (
            <PartFields name={`${parts}.${index}`} context={context} label={partOf(index)} />
          )}
        />
      </div>
    </>
  );
}

export function DatetimeFields({ name, label, context }: OpProps) {
  return (
    <>
      <PathField name={`${name}.from`} label="From" context={context} direction="from" />
      <StepsEditor name={`${name}.steps`} label={label} />
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the result is written."
      />
      <TextInput
        name={`${name}.input`}
        label="Input format"
        isRequired
        description="Tokens yyyy, yy, MM, dd, HH, mm, ss and separators, for example yyyyMMddHHmmss."
      />
      <TextInput
        name={`${name}.output`}
        label="Output format"
        isRequired
        description="The same tokens, for example yyyy-MM-dd HH:mm:ss."
      />
      <TextInput
        name={`${name}.fromTimezone`}
        label="From timezone"
        description="An IANA name such as Europe/Berlin. Set both timezones to convert."
      />
      <TextInput
        name={`${name}.toTimezone`}
        label="To timezone"
        description="Without both timezones the time is reformatted, not converted."
      />
    </>
  );
}

export function SequenceFields({ name, context }: OpProps) {
  return (
    <>
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the loop counter is written, for example $.1 for OBX-1."
      />
      <TextInput
        name={`${name}.start`}
        label="Start"
        isRequired
        validate={wholeNumber({ min: 0 })}
        description="The first number; the counter goes up by one for each occurrence."
      />
    </>
  );
}

/** The one-line explanation shown under each op (docs/spec/configuration.md §8.2). */
export const OP_HELP: Record<MappingOp, string> = {
  copy: "Copies the text of From to To. An empty or missing source uses Default; if that is empty too and Required is ticked, the message fails to transform.",
  constant: "Writes the Value as text to To.",
  lookup:
    "Looks the text of From up in a code table and writes the result to To. On missing says what to do with a code the table does not have.",
  concat:
    "Joins the parts in order, each the text at a path or literal text, and writes the result to To.",
  datetime:
    "Reformats a date or time from the input format to the output format. With both timezones set it also converts between them.",
  calculate:
    "Calculates a number with exact decimal arithmetic from values of the message and writes the rounded result to To. Check the formula against known values before relying on it.",
  when: "Runs the Then rules when the condition holds, otherwise the Else rules.",
  for_each:
    "For every occurrence of the record or segment in Over, runs the rules. By default each creates a new Into record; Rewrite in place changes that same record; Repetition of one field writes the next repetition of the field in Into. A path starting $. is the current input occurrence in From; in To it is the new record, the same record, or the next repetition.",
  append:
    "Adds one new record, built by the rules, after the existing ones, for example a calculated result. It is added only when every rule writes a value; otherwise nothing is added. In To, a path starting $. is the new record.",
  sequence:
    "Only inside a for_each or an append: writes the 1-based number of the record, for example OBX-1.",
};
