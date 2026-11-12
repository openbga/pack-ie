import { useCallback, useEffect, useMemo, useRef } from "react";
import { useLocation, useSearchParams } from "react-router";
import { type AuditView, parseAuditView, toAuditParams } from "./auditParams";

/** The open event's `seq` in the URL, or null; anything that is not a positive whole number is none. */
function parseSeq(value: string | null): number | null {
  if (value === null || !/^\d+$/.test(value)) return null;
  const seq = Number(value);
  return Number.isSafeInteger(seq) && seq > 0 ? seq : null;
}

/** What changes the filters: the next set, or one made from the set as it is by then. */
export type ViewChange = AuditView | ((current: AuditView) => AuditView);

const keyOf = (view: AuditView): string => new URLSearchParams(toAuditParams(view)).toString();

/**
 * The Audit filters and the open event, kept in the URL so Back, reload and a shared link show
 * the same events. A change replaces the history entry; changing the filters closes the open
 * event, which may no longer be in the list.
 *
 * A change made from a function is made from the filters as last asked for, which can be ahead
 * of the page: the address changes at once but the page follows a moment later, and a second
 * change in between (typing an actor right after choosing an action) must not undo the first.
 */
export function useAuditView(): {
  view: AuditView;
  setView: (change: ViewChange) => void;
  seq: number | null;
  setSeq: (seq: number | null) => void;
} {
  const [params, setParams] = useSearchParams();
  const { state } = useLocation();
  const view = useMemo(() => parseAuditView(params), [params]);
  const seq = parseSeq(params.get("seq"));
  const current = useRef(view);
  // The filters asked for that the page has not shown yet.
  const asked = useRef<AuditView | null>(null);
  useEffect(() => {
    current.current = view;
    if (asked.current !== null && keyOf(asked.current) === keyOf(view)) asked.current = null;
  }, [view]);
  const setView = useCallback(
    (change: ViewChange) => {
      const next = typeof change === "function" ? change(asked.current ?? current.current) : change;
      asked.current = next;
      setParams(new URLSearchParams(toAuditParams(next)), { replace: true, state });
    },
    [setParams, state],
  );
  const setSeq = useCallback(
    (next: number | null) =>
      setParams(
        (current) => {
          const updated = new URLSearchParams(current);
          if (next === null) updated.delete("seq");
          else updated.set("seq", String(next));
          return updated;
        },
        { replace: true, state },
      ),
    [setParams, state],
  );
  return { view, setView, seq, setSeq };
}
