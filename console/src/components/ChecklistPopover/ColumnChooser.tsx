import { ChecklistPopover } from "./ChecklistPopover";

interface ColumnChooserProps {
  /** The columns a person can show or hide. */
  options: readonly { id: string; label: string }[];
  /** The ids of `options` that are shown. */
  value: readonly string[];
  /** The columns chosen by hand; `null` is the default set, as many as fit. */
  chosen: readonly string[] | null;
  /** The default columns that fit the list's width. */
  fitting: readonly string[];
  /** How many columns the default set has, for saying how many did not fit. */
  defaultCount: number;
  onChange: (chosen: string[] | null) => void;
}

/** What the button says: the default set (and how many of it did not fit), or a custom one. */
function summaryOf(chosen: readonly string[] | null, fitting: readonly string[], count: number) {
  if (chosen !== null) return "Custom";
  const left = count - fitting.length;
  return left === 0 ? "Default" : `Default (${left} hidden to fit)`;
}

/** The "Columns" button of a grid: its checklist, and a reset to the default set. */
export function ColumnChooser({
  options,
  value,
  chosen,
  fitting,
  defaultCount,
  onChange,
}: ColumnChooserProps) {
  return (
    <ChecklistPopover
      label="Columns"
      summary={summaryOf(chosen, fitting, defaultCount)}
      options={options}
      value={value}
      onChange={onChange}
      resetLabel="Default columns"
      reset={{ isNeeded: chosen !== null, onReset: () => onChange(null) }}
    />
  );
}
