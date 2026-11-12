import { keepPreviousData, useInfiniteQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useEngineApi } from "@/api/context";
import { useEngineQuery } from "@/api/hooks";
import type { AuditEvent, AuditFilter, AuditPage, EngineError } from "@/api/types";
import { toEngineError } from "@/lib/errors";
import { type AuditView, toAuditFilter } from "./auditParams";

/** Events asked for at a time, at first and for each "Load older". */
const PAGE_SIZE = 100;

/** A page with the filter it was read with, so older pages count back from the same moment. */
interface ReadPage extends AuditPage {
  filter: AuditFilter;
}

interface PageParam {
  beforeSeq: number | null;
  /** Set on every page after the first. */
  filter: AuditFilter | null;
}

const FIRST: PageParam = { beforeSeq: null, filter: null };

/**
 * The events matching the view, newest first, a page at a time. A preset such as "Last 24
 * hours" counts back from the moment the first page is read (refresh reads it again). The list
 * is current whenever the page opens, and nothing is kept once it closes: details can hold
 * operator text.
 */
export function useAuditEvents(view: AuditView) {
  const api = useEngineApi();
  const query = useInfiniteQuery<
    ReadPage,
    EngineError,
    { pages: ReadPage[] },
    unknown[],
    PageParam
  >({
    queryKey: ["getAuditEvents", view],
    initialPageParam: FIRST,
    queryFn: async ({ pageParam }) => {
      const filter = pageParam.filter ?? toAuditFilter(view, Date.now());
      try {
        const page = await api.getAuditEvents({
          filter,
          beforeSeq: pageParam.beforeSeq,
          limit: PAGE_SIZE,
        });
        return { ...page, filter };
      } catch (error) {
        throw toEngineError(error);
      }
    },
    getNextPageParam: (last) =>
      last.nextBeforeSeq === null
        ? undefined
        : { beforeSeq: last.nextBeforeSeq, filter: last.filter },
    placeholderData: keepPreviousData,
    staleTime: 0,
    gcTime: 0,
  });
  const events = useMemo<readonly AuditEvent[] | undefined>(
    () => query.data?.pages.flatMap((page) => page.events),
    [query.data],
  );
  return { query, events };
}

/**
 * The open event when it is not among those loaded (a reload, a shared link): the event
 * itself is the newest one before `seq + 1`. `null` once the engine has no such event.
 */
export function useOpenedEvent(seq: number | null, loaded: readonly AuditEvent[] | undefined) {
  const inList = seq !== null && loaded?.some((event) => event.seq === seq) === true;
  return useEngineQuery(
    ["getAuditEvents", "event", seq],
    async (api) => {
      if (seq === null) return null;
      const page = await api.getAuditEvents({ filter: {}, beforeSeq: seq + 1, limit: 1 });
      return page.events.find((event) => event.seq === seq) ?? null;
    },
    { enabled: seq !== null && loaded !== undefined && !inList, staleTime: 0 },
  );
}
