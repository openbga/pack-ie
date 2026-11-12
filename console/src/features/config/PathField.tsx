import { TextInput } from "@/components/FormFields";
import type { RuleContext } from "./ruleForm";

interface PathFieldProps {
  name: string;
  label: string;
  context: RuleContext;
  /** `from` reads the message; `to` writes the result. */
  direction: "from" | "to";
  /** What the path is for, before the examples. */
  help?: string;
}

function example(context: RuleContext, direction: "from" | "to"): string {
  if (context === "loop") return "For example PID.3[0].1, R.3 or $.4 for the current occurrence.";
  if (context === "append") {
    return direction === "to"
      ? "A path starting $. in the new record, for example $.5."
      : "For example OBX[1].5 or R.4; the new record cannot be read here.";
  }
  return "For example PID.3[0].1, R.3 or col.sample_id.";
}

/** A path into the message; `$.` paths are offered only inside a `for_each` or an `append`. */
export function PathField({ name, label, context, direction, help }: PathFieldProps) {
  const text = example(context, direction);
  return (
    <TextInput name={name} label={label} isRequired description={help ? `${help} ${text}` : text} />
  );
}
