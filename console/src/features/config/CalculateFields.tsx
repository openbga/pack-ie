// The `calculate` rule: an exact decimal formula, written as text, and what to do when it cannot
// be calculated (docs/spec/configuration.md §8.2). An operand that needs more than a path (text
// steps, or the first record that matches a condition) is named, `@na`, and defined beside it.
import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import type { CalculateOnInvalid, CalculateRounding } from "@/api/types";
import { FieldList } from "@/components/FieldList";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import { Inset } from "@/components/Inset";
import { ConditionFields } from "./ConditionFields";
import styles from "./config.module.css";
import { FormCheckbox } from "./FormCheckbox";
import { formulaPaths, parseFormula, printFormula } from "./formulaText";
import { PathField } from "./PathField";
import type { OpProps } from "./RuleFields";
import type { RuleFormValues } from "./ruleForm";
import { StepsEditor } from "./StepFields";
import { emptyOperand, type OperandFormValues, valuesToOperand } from "./stepForm";
import { wholeNumber } from "./validators";

const ROUNDINGS: { id: CalculateRounding; label: string }[] = [
  { id: "half_up", label: "Half up: 2.345 becomes 2.35" },
  { id: "half_even", label: "Half even: 2.345 becomes 2.34" },
  { id: "truncate", label: "Truncate: 2.349 becomes 2.34" },
];

const COMPARATORS = [
  { id: "carry", label: "Calculate with the number and keep the < or >" },
  { id: "reject", label: "Not a number: treat as invalid" },
];

const NOTATIONS = [
  { id: "text", label: "Text: 1.5, <5" },
  { id: "sn", label: "HL7 structured numeric (SN): <^5" },
];

const ON_INVALID: { id: CalculateOnInvalid; label: string }[] = [
  { id: "error", label: "Fail the message" },
  { id: "skip", label: "Write nothing" },
  { id: "keep", label: "Write the value unchanged" },
  { id: "default", label: "Use the default" },
];

const OPERAND_NAME = /^[A-Za-z][A-Za-z0-9_]*$/;

/** The operands named in a rule, by name, as the engine's formula nodes. */
function operandsOf(values: readonly OperandFormValues[]) {
  return Object.fromEntries(
    values.map((operand) => [operand.name.trim(), valuesToOperand(operand)]),
  );
}

/** What the formula reads as, so the grouping of its brackets and the paths it reads are visible. */
function FormulaReading({ name }: { name: string }) {
  const { control } = useFormContext();
  const text = useWatch({ control, name: `${name}.formula` }) as string;
  const operands = useWatch({ control, name: `${name}.operands` }) as OperandFormValues[];
  const parsed = parseFormula(text, operandsOf(operands));
  if (!("formula" in parsed)) return null;
  const paths = formulaPaths(parsed.formula);
  const nameOf = (operand: unknown) =>
    operands.find((each) => JSON.stringify(valuesToOperand(each)) === JSON.stringify(operand))
      ?.name ?? "operand";
  return (
    <p role="status" className={`${styles.valid} ${styles.wide}`}>
      {`Reads as ${printFormula(parsed.formula, { pretty: true, nameOf })}.`}
      {paths.length > 0 && ` Takes ${paths.join(", ")} from the message.`}
    </p>
  );
}

const KINDS = [
  { id: "path", label: "The value at a path" },
  { id: "find", label: "The value in the first matching record" },
];

function OperandFields({ name, label }: { name: string; label: string }) {
  const { control, getValues } = useFormContext();
  const kind = useWatch({ control, name: `${name}.kind` }) as OperandFormValues["kind"];
  const operandsName = name.replace(/\.\d+$/, "");
  return (
    <>
      <TextInput
        name={`${name}.name`}
        label="Name"
        isRequired
        description="Written @name in the formula. Letters, digits and underscores, starting with a letter."
        validate={(value) => {
          if (!OPERAND_NAME.test(value))
            return "Use letters, digits and underscores, starting with a letter.";
          const all = (getValues(operandsName) ?? []) as OperandFormValues[];
          return all.filter((each) => each.name.trim() === value).length > 1
            ? "Another operand already has this name."
            : true;
        }}
      />
      <ChoiceInput name={`${name}.kind`} label="Reads" options={KINDS} isRequired />
      {kind === "find" && (
        <>
          <TextInput
            name={`${name}.find`}
            label="Record or segment"
            isRequired
            description="The record or segment to search, for example OBX. The first one that meets the condition is used; none is an empty value."
          />
          <div className={styles.nested}>
            <Inset as="fieldset">
              <legend className={styles.legend}>Condition of {label.toLowerCase()}</legend>
              <ConditionFields
                prefix={`${name}.where`}
                label={`Condition of ${label.toLowerCase()}`}
                inLoop
              />
            </Inset>
          </div>
        </>
      )}
      <TextInput
        name={`${name}.from`}
        label={kind === "find" ? "Value path" : "From"}
        isRequired
        description={
          kind === "find"
            ? "A path starting $. in the record found, for example $.5."
            : "A path of the message, for example OBX[1].5 or R.4."
        }
      />
      <StepsEditor name={`${name}.steps`} label={label} />
    </>
  );
}

/** Named operands: the values a formula reads that need more than a plain path. */
function OperandsEditor({ name, label }: { name: string; label: string }) {
  const { control, getValues } = useFormContext();
  const operands = `${name}.operands`;
  const { fields, append, remove } = useFieldArray({ control, name: operands });
  const owner = label.toLowerCase();
  return (
    <div className={styles.nested}>
      <FieldList
        name={operands}
        legend={`Operands of ${owner}`}
        description="Name a value that needs text steps or a search, then write it @name in the formula, for example @na - (@cl + @hco3). A plain path needs no operand."
        emptyText="No operands."
        items={fields}
        itemLabel={(index) => `Operand ${index + 1} of ${owner}`}
        addLabel={`Add operand to ${owner}`}
        onAdd={() => {
          const taken = new Set(
            ((getValues(operands) ?? []) as OperandFormValues[]).map((each) => each.name),
          );
          let n = taken.size + 1;
          while (taken.has(`value${n}`)) n += 1;
          append(emptyOperand(`value${n}`));
        }}
        onRemove={remove}
        renderItem={(index) => (
          <OperandFields name={`${operands}.${index}`} label={`Operand ${index + 1} of ${owner}`} />
        )}
      />
    </div>
  );
}

/** Whether a formula has exactly one operand, which `keep` needs (it writes that text back). */
function oneOperand(formula: string, operands: RuleFormValues["operands"]): boolean {
  const parsed = parseFormula(formula, operandsOf(operands));
  if (!("formula" in parsed)) return true;
  const count = (node: Parameters<typeof formulaPaths>[0]): number =>
    "op" in node ? node.args.reduce((sum, arg) => sum + count(arg), 0) : "constant" in node ? 0 : 1;
  return count(parsed.formula) === 1;
}

/** Whether both ends speak HL7, which a structured numeric needs. */
function useBothHl7(root: string): boolean {
  const { control } = useFormContext();
  const source = useWatch({ control, name: "source.protocol" });
  const destination = useWatch({ control, name: `${root}.protocol` });
  return source === "hl7_mllp" && destination === "hl7_mllp";
}

export function CalculateFields({ name, label, context, root }: OpProps) {
  const { control, getValues } = useFormContext();
  const hl7 = useBothHl7(root);
  const notation = useWatch({ control, name: `${name}.notation` }) as string;
  const scale = useWatch({ control, name: `${name}.scale` }) as string;
  const onInvalid = useWatch({ control, name: `${name}.onInvalid` }) as CalculateOnInvalid;
  const exact = scale.trim() === "";
  const example = context === "top" ? "OBX[1].5 / 88.4" : "$.4 * 0.0555";
  return (
    <>
      <TextInput
        name={`${name}.formula`}
        label="Formula"
        isRequired
        description={`Exact decimal arithmetic with + - * / and brackets, and min(a, b), max(a, b), abs(a). Numbers are written as typed; a path is read from the message. For example ${example}.`}
        validate={(value) => {
          const operands = (getValues(`${name}.operands`) ?? []) as OperandFormValues[];
          const parsed = parseFormula(value, operandsOf(operands));
          return "error" in parsed ? parsed.error : true;
        }}
      />
      <FormulaReading name={name} />
      <OperandsEditor name={name} label={label} />
      <PathField
        name={`${name}.to`}
        label="To"
        context={context}
        direction="to"
        help="Where the result is written; it may be a path the formula reads."
      />
      <TextInput
        name={`${name}.scale`}
        label="Decimal places"
        description="0 to 12. The result is rounded to this many places and written with exactly that many, so 5.5 with 2 places is 5.50. Leave empty to write the exact result; a division needs it."
        validate={(value) => wholeNumber({ min: 0, max: 12 })(value)}
      />
      <ChoiceInput
        name={`${name}.rounding`}
        label="Rounding"
        options={ROUNDINGS}
        isDisabled={exact}
        description={exact ? "Used when decimal places are set." : undefined}
      />
      {(hl7 || notation === "sn") && (
        <ChoiceInput
          name={`${name}.notation`}
          label="Number written as"
          options={NOTATIONS}
          description="Text is a plain number, with a sign such as <5 in front. A structured numeric keeps its sign and its number in components 1 and 2 of the field: <^5. Use a path to the field, such as $.5, not to a component; the result is written back as components."
          validate={(value) =>
            value !== "sn" ||
            hl7 ||
            "A structured numeric needs HL7 on both the source and this destination."
          }
        />
      )}
      <ChoiceInput
        name={`${name}.comparator`}
        label="A value like <5 or >1000"
        options={COMPARATORS}
        description="Carry calculates with the number and puts the sign before the result, rounded so that it stays true: <5 times 2 is <10; multiplying by 0 gives 0. One such value per formula, not inside min, max or abs and not as a divisor."
      />
      <ChoiceInput
        name={`${name}.onInvalid`}
        label="When it cannot be calculated"
        options={ON_INVALID}
        isRequired
        description="A value that is not a number, a division by zero, a result beyond the supported range. Writing the value unchanged needs a formula with exactly one operand."
        validate={(value, values) => {
          if (value !== "keep") return true;
          const rule = name
            .split(".")
            .reduce<unknown>(
              (at, key) => (at as Record<string, unknown>)?.[key],
              values,
            ) as RuleFormValues;
          return (
            oneOperand(rule.formula, rule.operands) ||
            "Writing the value unchanged needs a formula with exactly one operand."
          );
        }}
      />
      {onInvalid === "default" && (
        <TextInput
          name={`${name}.default`}
          label="Default"
          isRequired
          description="Written as text when the calculation cannot be done."
        />
      )}
      <FormCheckbox
        name={`${name}.required`}
        label="Required"
        description="An empty or missing value, or a record not found, fails the message instead of writing nothing."
      />
    </>
  );
}
