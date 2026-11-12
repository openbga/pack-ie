import { DateTimeField } from "@/components/DateTimeField";
import { Select } from "@/components/Select";

interface TimeRangeFieldsProps<Range extends string> {
  /** "Time range" for a filter; "Period" for a report. */
  label?: string;
  /** `inline` in a toolbar; a form keeps the stacked label. */
  layout?: "inline";
  ranges: readonly { id: Range; label: string }[];
  range: Range;
  /** Custom range only: RFC 3339 times. */
  from: string | undefined;
  to: string | undefined;
  onRange: (range: Range) => void;
  onFrom: (from: string | undefined) => void;
  onTo: (to: string | undefined) => void;
}

/** The range choice, and From and To after it while the range is "custom". */
export function TimeRangeFields<Range extends string>({
  label = "Time range",
  layout,
  ranges,
  range,
  from,
  to,
  onRange,
  onFrom,
  onTo,
}: TimeRangeFieldsProps<Range>) {
  return (
    <>
      <Select
        layout={layout}
        label={label}
        name="range"
        options={ranges}
        value={range}
        onChange={(next) => {
          const chosen = ranges.find((each) => each.id === next);
          if (chosen) onRange(chosen.id);
        }}
      />
      {range === "custom" && (
        <>
          <DateTimeField label="From" value={from} onCommit={onFrom} />
          <DateTimeField label="To" value={to} onCommit={onTo} />
        </>
      )}
    </>
  );
}
