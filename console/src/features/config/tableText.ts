// A code table as lines of text, so a table of a hundred codes can be pasted from a spreadsheet,
// copied out or edited in bulk: one `code,text` (or `code<TAB>text`) per line. The text may hold
// commas; a value that starts with `"` is quoted, with `""` for a quote.
import type { CodeTableFormValues } from "./ruleForm";

type Row = CodeTableFormValues["rows"][number];

interface TextProblem {
  /** 1-based line of the pasted text. */
  line: number;
  message: string;
}

export interface ParsedRows {
  rows: Row[];
  problems: TextProblem[];
}

const quote = (value: string) => `"${value.replaceAll('"', '""')}"`;

/** One value as text: quoted when it would not read back the same. */
function field(value: string, separators: RegExp): string {
  return separators.test(value) || value !== value.trim() ? quote(value) : value;
}

export function rowsToText(rows: readonly Row[]): string {
  return rows
    .map((row) => `${field(row.code, /[,\t"\n\r]/)},${field(row.text, /^"|[\t\n\r]/)}`)
    .join("\n");
}

/** A leading quoted value and what follows it, or null when the line does not start with one. */
function quoted(line: string): { value: string; rest: string } | null {
  if (!line.startsWith('"')) return null;
  let value = "";
  for (let i = 1; i < line.length; i += 1) {
    if (line[i] !== '"') value += line[i];
    else if (line[i + 1] === '"') {
      value += '"';
      i += 1;
    } else return { value, rest: line.slice(i + 1) };
  }
  return null;
}

/** Splits a line at its first tab, or else its first comma, outside quotes. */
function splitLine(line: string): [string, string] | null {
  const lead = quoted(line);
  if (lead) {
    const rest = lead.rest.trimStart();
    if (!rest.startsWith(",") && !rest.startsWith("\t")) return null;
    const text = rest.slice(1).trim();
    const second = quoted(text);
    return [lead.value, second && second.rest.trim() === "" ? second.value : text];
  }
  if (line.startsWith('"')) return null;
  const at = line.includes("\t") ? line.indexOf("\t") : line.indexOf(",");
  if (at < 0) return null;
  const text = line.slice(at + 1).trim();
  const second = quoted(text);
  return [line.slice(0, at).trim(), second && second.rest.trim() === "" ? second.value : text];
}

export function textToRows(text: string): ParsedRows {
  const rows: Row[] = [];
  const problems: TextProblem[] = [];
  const seen = new Map<string, number>();
  for (const [index, raw] of text.split(/\r\n|\n|\r/).entries()) {
    if (raw.trim() === "") continue;
    const line = index + 1;
    const parts = splitLine(raw);
    if (parts === null) {
      problems.push({ line, message: "Separate the code from its text with a comma or a tab." });
      continue;
    }
    const [code, replacement] = parts;
    if (code === "") {
      problems.push({ line, message: "The code before the separator is empty." });
    } else if (seen.has(code)) {
      problems.push({ line, message: `The code ${code} is already on line ${seen.get(code)}.` });
    } else {
      seen.set(code, line);
      rows.push({ code, text: replacement });
    }
  }
  return { rows, problems };
}

export interface RowChanges {
  added: number;
  changed: number;
  removed: number;
  unchanged: number;
}

/** What replacing `before` by `after` does, by code. */
export function compareRows(before: readonly Row[], after: readonly Row[]): RowChanges {
  const was = new Map(before.map((row) => [row.code.trim(), row.text]));
  const now = new Map(after.map((row) => [row.code, row.text]));
  const changes: RowChanges = { added: 0, changed: 0, removed: 0, unchanged: 0 };
  for (const [code, text] of now) {
    if (!was.has(code)) changes.added += 1;
    else if (was.get(code) === text) changes.unchanged += 1;
    else changes.changed += 1;
  }
  for (const code of was.keys()) if (!now.has(code)) changes.removed += 1;
  return changes;
}
