import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";

interface NoResultsProps {
  /** What the list holds, plural and lower case, such as "analytes". */
  noun: string;
  /** The find text; blank when only filters exclude every record. */
  query: string;
  /** Clears the find text, or the filters when there is none. */
  onClear: () => void;
}

/**
 * The find or the filters exclude every record: "No analytes match 'oxy'" with "Clear
 * search". Not for a list with nothing in it yet, which is an `EmptyState` with its Add action.
 */
export function NoResults({ noun, query, onClear }: NoResultsProps) {
  const searched = query.trim() !== "";
  return (
    <EmptyState
      title={searched ? `No ${noun} match '${query.trim()}'` : `No ${noun} match the filters`}
      action={<Button onPress={onClear}>{searched ? "Clear search" : "Clear filters"}</Button>}
    />
  );
}
