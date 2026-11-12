import { useCallback, useMemo } from "react";
import { useLocation, useSearchParams } from "react-router";
import {
  parseTransmissionView,
  type TransmissionView,
  toTransmissionParams,
} from "./transmissionParams";

/**
 * The report form, kept in the URL so a link, Back and reload show the same form. A change
 * replaces the history entry. It does not run the report.
 */
export function useTransmissionView(): {
  view: TransmissionView;
  setView: (view: TransmissionView) => void;
} {
  const [params, setParams] = useSearchParams();
  const { state } = useLocation();
  const view = useMemo(() => parseTransmissionView(params), [params]);
  const setView = useCallback(
    (next: TransmissionView) =>
      setParams(new URLSearchParams(toTransmissionParams(next)), { replace: true, state }),
    [setParams, state],
  );
  return { view, setView };
}
