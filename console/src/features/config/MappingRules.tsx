// The mapping rule list, which nests: a `when` holds Then and Else lists and a `for_each` a
// list of its own, each rendered by this same component (docs/spec/configuration.md §8.2).
import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import type { MappingOp } from "@/api/types";
import { FieldList } from "@/components/FieldList";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import { Inset } from "@/components/Inset";
import { CalculateFields } from "./CalculateFields";
import { ConditionFields } from "./ConditionFields";
import styles from "./config.module.css";
import { nextId } from "./configDocument";
import {
  ConcatFields,
  ConstantFields,
  CopyFields,
  DatetimeFields,
  LookupFields,
  OP_HELP,
  SequenceFields,
} from "./RuleFields";
import { emptyRule, type RuleContext, type RuleFormValues, ruleIds } from "./ruleForm";

const LEAF_OPS: MappingOp[] = ["copy", "constant", "lookup", "concat", "datetime", "calculate"];

/** The rules a list offers: loops and appended records hold leaf rules, which no loop nests. */
const OPS: Record<RuleContext, MappingOp[]> = {
  top: [...LEAF_OPS, "when", "for_each", "append"],
  loop: [...LEAF_OPS, "when", "sequence"],
  append: [...LEAF_OPS, "sequence"],
};

/** Where a list sits, which decides how its rules are named and what they may be. */
export interface ListScope {
  /** The destination the rules belong to, such as `destinations.0`; holds `mappings` and `codeTables`. */
  root: string;
  /** Names that destination in labels: `Destination 1`. */
  owner: string;
  /** `Destination 1 mapping rule`, `Destination 1 loop rule`, `Destination 1 then rule`, ... */
  word: string;
  /** The number of the rule that holds the list, with a dot: `2.`; empty at the top. */
  parentNumber: string;
  /** Names the list: "Rules inside mapping rule 2". */
  legend: string;
  description: string;
  addLabel: string;
  context: RuleContext;
}

/** The top-level rules of one destination; `owner` names it in every label. */
export function topScope(root: string, owner: string): ListScope {
  return {
    root,
    owner,
    word: `${owner} mapping rule`,
    parentNumber: "",
    legend: `${owner} mapping rules`,
    description:
      "Rules run from top to bottom and build the message sent to this destination. Leave empty to pass the original message through unchanged; the destination's protocol must then be the source's.",
    addLabel: `Add ${owner.toLowerCase()} mapping rule`,
    context: "top",
  };
}

/** The lists a rule can hold: a loop's rules, or the rules for a condition that holds or not. */
type ListKind = "loop" | "append" | "when-true" | "when-false";

const LIST_TEXT: Record<
  ListKind,
  { word: string; lead: string; add: string; description: string }
> = {
  loop: {
    word: "Loop rule",
    lead: "Rules inside",
    add: "Add rule inside",
    description:
      "Run once for every occurrence. A path starting $. is the current input occurrence in From. In To it is the new record, the same record when rewriting in place, or the next repetition of the field when writing repetitions.",
  },
  append: {
    word: "Appended rule",
    lead: "Rules of the record added by",
    add: "Add rule to the record added by",
    description:
      "The record is added only when every rule writes a value. A To path starting $. is the new record; From paths address the message, and a find operand may use $. for the record it checks.",
  },
  "when-true": {
    word: "Then rule",
    lead: "Then rules of",
    add: "Add then rule to",
    description: "Run when the condition holds.",
  },
  "when-false": {
    word: "Else rule",
    lead: "Else rules of",
    add: "Add else rule to",
    description: "Run when the condition does not hold. Leave empty to do nothing.",
  },
};

function scopeIn(
  parent: { label: string; number: string; context: RuleContext; root: string; owner: string },
  kind: ListKind,
): ListScope {
  const text = LIST_TEXT[kind];
  const label = parent.label.toLowerCase();
  return {
    root: parent.root,
    owner: parent.owner,
    word: `${parent.owner} ${text.word.toLowerCase()}`,
    parentNumber: `${parent.number}.`,
    legend: `${text.lead} ${label}`,
    description: text.description,
    addLabel: `${text.add} ${label}`,
    context: kind === "loop" || kind === "append" ? kind : parent.context,
  };
}

interface MappingRulesProps {
  /** The form path of the list: `destinations.0.mappings` or `destinations.0.mappings.2.then`. */
  name: string;
  scope: ListScope;
}

/** One ordered list of mapping rules, with Add, Move up, Move down and Remove at every level. */
export function MappingRules({ name, scope }: MappingRulesProps) {
  const { control, getValues } = useFormContext();
  const { fields, append, remove, move } = useFieldArray({ control, name });
  const numberOf = (index: number) => `${scope.parentNumber}${index + 1}`;
  const labelOf = (index: number) => `${scope.word} ${numberOf(index)}`;
  return (
    <FieldList
      name={name}
      legend={scope.legend}
      description={scope.description}
      emptyText="No rules."
      items={fields}
      itemLabel={labelOf}
      addLabel={scope.addLabel}
      onAdd={() => {
        const taken = ruleIds((getValues(`${scope.root}.mappings`) ?? []) as RuleFormValues[]);
        append(emptyRule(nextId("m", taken)));
      }}
      onMove={move}
      onRemove={remove}
      renderItem={(index) => (
        <MappingRuleFields
          name={`${name}.${index}`}
          label={labelOf(index)}
          number={numberOf(index)}
          context={scope.context}
          root={scope.root}
          owner={scope.owner}
        />
      )}
    />
  );
}

interface RuleProps {
  /** The form path of the rule. */
  name: string;
  label: string;
  /** The rule's number, such as `2.1`. */
  number: string;
  context: RuleContext;
  /** The destination holding the rule and the name that labels use for it. */
  root: string;
  owner: string;
}

function WhenFields({ name, label, number, context, root, owner }: RuleProps) {
  const parent = { label, number, context, root, owner };
  return (
    <>
      <div className={styles.nested}>
        <Inset as="fieldset">
          <legend className={styles.legend}>Condition of {label.toLowerCase()}</legend>
          <ConditionFields
            prefix={`${name}.condition`}
            label={`Condition of ${label.toLowerCase()}`}
            inLoop={context === "loop"}
          />
        </Inset>
      </div>
      <div className={styles.nested}>
        <MappingRules name={`${name}.then`} scope={scopeIn(parent, "when-true")} />
      </div>
      <div className={styles.nested}>
        <MappingRules name={`${name}.else`} scope={scopeIn(parent, "when-false")} />
      </div>
    </>
  );
}

const MODES = [
  { id: "create", label: "Create a new record for each one found" },
  { id: "update", label: "Rewrite each record found in place" },
  { id: "repeat", label: "Write each one as a repetition of one field" },
];

/** Whether both ends speak the same HL7 or ASTM, so the target starts as a copy of the source. */
function useCanUpdate(root: string): boolean {
  const { control } = useFormContext();
  const source = useWatch({ control, name: "source.protocol" });
  const destination = useWatch({ control, name: `${root}.protocol` });
  return source === destination && (source === "hl7_mllp" || source === "astm");
}

/** Whether the destination is HL7 or ASTM, the targets a repetition can be written into. */
function useCanRepeat(root: string): boolean {
  const { control } = useFormContext();
  const destination = useWatch({ control, name: `${root}.protocol` });
  return destination === "hl7_mllp" || destination === "astm";
}

const MODE_HELP = {
  create:
    "Each record found creates a new one; a path starting $. then means the same record in From and the new one in To.",
  update:
    "Rewriting changes the records of the copy this destination starts from, so the count stays the same; a path starting $. then means the same record in From and To.",
  repeat:
    "Each record found writes one repetition of the field in Into, in order: a To of $.5.4 is component 4 of the next repetition of field 5. Use it to list the tests of an order in one field.",
};

function ForEachFields({ name, label, number, context, root, owner }: RuleProps) {
  const { control } = useFormContext();
  const mode = useWatch({ control, name: `${name}.mode` }) as "create" | "update" | "repeat";
  const canUpdate = useCanUpdate(root);
  const canRepeat = useCanRepeat(root);
  return (
    <>
      <ChoiceInput
        name={`${name}.mode`}
        label="Records"
        options={MODES}
        isRequired
        description={MODE_HELP[mode]}
        validate={(value) => {
          if (value === "update" && !canUpdate) {
            return "Rewriting in place needs the same protocol, HL7 or ASTM, on the source and this destination.";
          }
          return (
            value !== "repeat" ||
            canRepeat ||
            "Writing repetitions needs an HL7 or ASTM destination."
          );
        }}
      />
      <TextInput
        name={`${name}.over`}
        label="Over"
        isRequired
        description="The record or segment to repeat for, for example R."
      />
      {mode === "create" && (
        <TextInput
          name={`${name}.into`}
          label="Into"
          isRequired
          description="The record or segment to create for each one, for example OBX."
        />
      )}
      {mode === "repeat" && (
        <TextInput
          name={`${name}.into`}
          label="Into field"
          isRequired
          description="The field to write the repetitions into, such as O.5 for field 5 of the first O record; no repetition and no component."
        />
      )}
      <div className={styles.nested}>
        <MappingRules
          name={`${name}.rules`}
          scope={scopeIn({ label, number, context, root, owner }, "loop")}
        />
      </div>
    </>
  );
}

function AppendFields({ name, label, number, context, root, owner }: RuleProps) {
  return (
    <>
      <TextInput
        name={`${name}.into`}
        label="Into"
        isRequired
        description="The record or segment to add, for example OBX. It is added after the existing ones."
      />
      <div className={styles.nested}>
        <MappingRules
          name={`${name}.rules`}
          scope={scopeIn({ label, number, context, root, owner }, "append")}
        />
      </div>
    </>
  );
}

function OpFields({ op, ...rule }: RuleProps & { op: MappingOp }) {
  switch (op) {
    case "copy":
      return <CopyFields {...rule} />;
    case "constant":
      return <ConstantFields {...rule} />;
    case "lookup":
      return <LookupFields {...rule} />;
    case "concat":
      return <ConcatFields {...rule} />;
    case "datetime":
      return <DatetimeFields {...rule} />;
    case "calculate":
      return <CalculateFields {...rule} />;
    case "when":
      return <WhenFields {...rule} />;
    case "for_each":
      return <ForEachFields {...rule} />;
    case "append":
      return <AppendFields {...rule} />;
    case "sequence":
      return <SequenceFields {...rule} />;
  }
}

/** One rule: its operator, a line on what the operator does, and the operator's fields. */
function MappingRuleFields(rule: RuleProps) {
  const { control } = useFormContext();
  const op = useWatch({ control, name: `${rule.name}.op` }) as MappingOp;
  // `sequence` counts the loop, so it is only offered inside one; a rule that already is one
  // stays visible so the engine's complaint has something to point at.
  const offered = OPS[rule.context].includes(op) ? OPS[rule.context] : [...OPS[rule.context], op];
  return (
    <>
      <ChoiceInput
        name={`${rule.name}.op`}
        label="Rule"
        options={offered.map((id) => ({ id, label: id }))}
        isRequired
        description={OP_HELP[op]}
      />
      <OpFields op={op} {...rule} />
    </>
  );
}
