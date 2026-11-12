import { useCallback, useMemo } from "react";
import { useSearchParams } from "react-router";
import { type LogFilters, parseLogFilters, toLogSearchParams } from "./logParams";

/**
 * The Logs filters, kept in the URL so Back, reload and the links from channel and endpoint
 * pages show the same events. A change replaces the history entry.
 */
export function useLogFilters(): [filters: LogFilters, setFilters: (filters: LogFilters) => void] {
  const [params, setParams] = useSearchParams();
  const filters = useMemo(() => parseLogFilters(params), [params]);
  const setFilters = useCallback(
    (next: LogFilters) => setParams(toLogSearchParams(next), { replace: true }),
    [setParams],
  );
  return [filters, setFilters];
}
