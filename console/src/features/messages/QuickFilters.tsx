import { keepPreviousData } from "@tanstack/react-query";
import { useEngineQuery } from "@/api/hooks";
import { ToggleChip } from "@/components/ToggleChip";
import { formatCount } from "@/lib/format";
import {
  type MessageFilters,
  QUICK_FILTERS,
  QUICK_ORDER,
  type QuickFilter,
  quickCountQuery,
} from "./messageParams";
import styles from "./QuickFilters.module.css";

interface QuickFiltersProps {
  filters: MessageFilters;
  /** The quick filter to turn on, or `undefined` to turn the current one off. */
  onChange: (quick: QuickFilter | undefined) => void;
}

/** How often the counts are read again while the page is open. */
const COUNT_REFRESH_MS = 10_000;

/** The filters a count depends on: every filter but the quick filter and the state choices. */
function countKey(filters: MessageFilters) {
  const { quick: _quick, state: _state, delivery: _delivery, ...rest } = filters;
  return rest;
}

interface ChipProps {
  quick: QuickFilter;
  filters: MessageFilters;
  onChange: QuickFiltersProps["onChange"];
}

/**
 * One quick filter with its count. The engine gives no count of its own, so the count is the
 * total of a one-row query with the current filters and the quick filter's criteria; a count
 * that cannot be read is left out rather than shown as zero.
 */
function QuickChip({ quick, filters, onChange }: ChipProps) {
  const selected = filters.quick === quick;
  const count = useEngineQuery(
    ["listMessages", "count", quick, countKey(filters)],
    (api) => api.listMessages(quickCountQuery(filters, quick, Date.now())),
    { placeholderData: keepPreviousData, refetchInterval: COUNT_REFRESH_MS },
  );
  const total = count.data?.total;
  return (
    <ToggleChip isSelected={selected} onChange={(next) => onChange(next ? quick : undefined)}>
      {QUICK_FILTERS[quick].label}{" "}
      {total !== undefined && <span className={styles.count}>{formatCount(total)}</span>}
    </ToggleChip>
  );
}

/** The quick filters of the Messages list with their counts; one at a time. */
export function QuickFilters({ filters, onChange }: QuickFiltersProps) {
  return (
    <fieldset className={styles.quick}>
      <legend className="visually-hidden">Quick filters</legend>
      {QUICK_ORDER.map((quick) => (
        <QuickChip key={quick} quick={quick} filters={filters} onChange={onChange} />
      ))}
    </fieldset>
  );
}
