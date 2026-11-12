import { useCallback, useMemo, useState } from "react";
import { useLocation, useSearchParams } from "react-router";
import {
  type MessageView,
  parseMessageView,
  SENSITIVE_KEYS,
  type SensitiveFilters,
  toSearchParams,
} from "./messageParams";
import { keepMessagesSession, messagesSession } from "./messagesSession";

/** Only the criteria that are set, so unset ones are absent rather than `undefined`. */
function criteriaOf(filters: SensitiveFilters): SensitiveFilters {
  const set: SensitiveFilters = {};
  for (const key of SENSITIVE_KEYS) {
    const value = filters[key];
    if (value) Object.assign(set, { [key]: value });
  }
  return set;
}

/**
 * The list's filters, sort and page. All but the identifier, text and message ID criteria
 * live in the URL, so Back, reload and a link from a channel page show the same rows. Those
 * can hold a patient identifier or message content, which does not belong in an address: they
 * are kept in memory for the console session (`messagesSession`), so opening a message and
 * coming back finds them as they were, and a reload forgets them. A change replaces the
 * history entry, so typing and paging do not leave one entry each, and keeps the navigation
 * state.
 */
export function useMessageView(): [view: MessageView, setView: (view: MessageView) => void] {
  const [params, setParams] = useSearchParams();
  const { state } = useLocation();
  const [criteria, setCriteria] = useState<SensitiveFilters>(() => messagesSession().criteria);
  const parsed = useMemo(() => parseMessageView(params), [params]);
  const view = useMemo<MessageView>(
    () => ({ ...parsed, filters: { ...parsed.filters, ...criteria } }),
    [parsed, criteria],
  );
  const setView = useCallback(
    (next: MessageView) => {
      const kept = criteriaOf(next.filters);
      const rest = { ...next.filters };
      for (const key of SENSITIVE_KEYS) delete rest[key];
      keepMessagesSession({ criteria: kept });
      setCriteria(kept);
      setParams(toSearchParams({ ...next, filters: rest }), { replace: true, state });
    },
    [setParams, state],
  );
  return [view, setView];
}
