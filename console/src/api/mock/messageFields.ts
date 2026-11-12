import type { MessageField, MessageFields, Protocol } from "../types";
import { STX } from "./messageBodies";

// ── Parsed fields ────────────────────────────────────────────────────────────────────────

/** The engine lists at most this many fields (`MAX_FIELDS`). */
const MAX_FIELDS = 500;

/** How a protocol splits a field: repetitions, components, subcomponents (none in ASTM). */
interface Splits {
  repeat: string;
  component: string;
  sub: string | null;
}

/** `PID`, then `PID[1]` for the second one: the engine counts occurrences from zero. */
function occurrenceName(name: string, seen: Map<string, number>): string {
  const count = seen.get(name) ?? 0;
  seen.set(name, count + 1);
  return count === 0 ? name : `${name}[${count}]`;
}

/**
 * The leaves of one field as the engine lists them: the deepest level the field has, `[n]` for
 * several repetitions, empty values left out.
 */
function fieldLeaves(prefix: string, value: string, splits: Splits): MessageField[] {
  const repetitions = value
    .split(splits.repeat)
    .map((repetition) =>
      repetition
        .split(splits.component)
        .map((component) => component.split(splits.sub ?? "\u0000")),
    );
  const several = repetitions.length > 1;
  return repetitions.flatMap((components, r) => {
    const only = components.length === 1 && components[0]?.length === 1;
    if (only)
      return [{ path: several ? `${prefix}[${r}]` : prefix, value: components[0]?.[0] ?? "" }];
    const repetition = r === 0 ? "" : `[${r}]`;
    return components.flatMap((subs, c) => {
      const base = `${prefix}${repetition}.${c + 1}`;
      if (subs.length === 1) return [{ path: base, value: subs[0] ?? "" }];
      return subs.map((sub, k) => ({ path: `${base}.${k + 1}`, value: sub }));
    });
  });
}

/**
 * One record's non-empty leaves, `first` being the path number of `parts[1]`. The part after
 * the name of MSH and H holds the delimiters, which are not listed.
 */
function recordLeaves(
  name: string,
  parts: string[],
  first: number,
  splits: Splits,
  hasDelimiters: boolean,
): MessageField[] {
  return parts.flatMap((part, i) => {
    if (i === 0 || (hasDelimiters && i === 1)) return [];
    return fieldLeaves(`${name}.${first + i - 1}`, part, splits).filter((f) => f.value !== "");
  });
}

function hl7Fields(text: string): MessageField[] | null {
  const lines = text.split("\r").filter((line) => line !== "");
  if (!lines[0]?.startsWith("MSH|")) return null;
  const splits: Splits = { repeat: "~", component: "^", sub: "&" };
  const seen = new Map<string, number>();
  return lines.flatMap((line) => {
    const parts = line.split("|");
    const name = occurrenceName(parts[0] ?? "", seen);
    // MSH-1 is the separator, so the part after MSH is MSH.2 and the next is MSH.3.
    const first = parts[0] === "MSH" ? 2 : 1;
    return recordLeaves(name, parts, first, splits, parts[0] === "MSH");
  });
}

function astmFields(text: string): MessageField[] | null {
  const records = text
    .split(STX)
    .slice(1)
    .map((frame) => frame.slice(1).split("\r")[0] ?? "");
  const known = new Set(["H", "P", "O", "R", "C", "M", "Q", "S", "L"]);
  if (!records[0]?.startsWith("H|") || records.some((r) => !known.has(r[0] ?? ""))) return null;
  const splits: Splits = { repeat: "\\", component: "^", sub: null };
  const seen = new Map<string, number>();
  return records.flatMap((record) => {
    const parts = record.split("|");
    const name = occurrenceName(parts[0] ?? "", seen);
    return recordLeaves(name, parts, 2, splits, parts[0] === "H");
  });
}

/**
 * The fields the engine would list for an original (`GetMessage`'s `fields`): HL7 and ASTM by
 * the paths filters use, a reason in words for what does not parse (`parses` false: the
 * message is invalid) or has no field list.
 */
export function parsedFields(text: string, protocol: Protocol, parses = true): MessageFields {
  const none = (unavailable: string): MessageFields => ({
    fields: [],
    truncated: false,
    unavailable,
  });
  if (protocol !== "hl7_mllp" && protocol !== "astm") {
    return none("Fields are listed for HL7 and ASTM messages.");
  }
  const all = !parses ? null : protocol === "hl7_mllp" ? hl7Fields(text) : astmFields(text);
  if (all === null) {
    const name = protocol === "hl7_mllp" ? "HL7" : "ASTM";
    return none(`The original does not parse as ${name}; see the message's reason.`);
  }
  return {
    fields: all.slice(0, MAX_FIELDS),
    truncated: all.length > MAX_FIELDS,
    unavailable: null,
  };
}
