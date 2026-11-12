import { keepPreviousData } from "@tanstack/react-query";
import { useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { useEngineQuery, useSnapshot } from "@/api/hooks";
import type { ChannelSnapshot, MessagePage, MessageSummary, StoreStatus } from "@/api/types";
import { Button } from "@/components/Button";
import { ColumnChooser } from "@/components/ChecklistPopover";
import { DataGrid, type GridRowActions, type GridSort } from "@/components/DataGrid";
import { EmptyState } from "@/components/EmptyState";
import { LoadingState } from "@/components/LoadingState";
import { Page, PageHeader } from "@/components/Page";
import { PagingBar } from "@/components/PagingBar";
import { QueryContent } from "@/components/QueryContent";
import { RefreshControl } from "@/components/RefreshControl";
import { useElementWidth } from "@/lib/useElementWidth";
import { DEFAULT_ORDER, fittingColumns } from "./columnFit";
import { ExportDialog } from "./ExportDialog";
import { filterWords } from "./filterWords";
import { MessageFilterBar } from "./MessageFilterBar";
import { SampleDataNotice, StoreNotAvailable, storeProblem } from "./MessageStoreNotice";
import styles from "./MessagesPage.module.css";
import { COLUMN_CHOICES, choosableIn, identifierOf, messageColumns } from "./messageColumns";
import {
  DEFAULT_FILTERS,
  DEFAULT_SORT,
  isAnyTime,
  isNarrowed,
  isSortField,
  type MessageFilters,
  type MessageView,
  PAGE_SIZES,
  toMessageFilter,
  toMessageQuery,
} from "./messageParams";
import { keepMessagesSession, messagesSession } from "./messagesSession";
import { canResend } from "./messageWording";
import type { MessageListState } from "./navigation";
import { ResendDialog } from "./ResendDialog";
import { subjectOfSummary } from "./resendRules";
import { useMessageView } from "./useMessageView";

const getRowId = (message: MessageSummary) => message.id;

/** The channels whose messages the store holds: message channels and bridges that record. */
function storedChannels(channels: readonly ChannelSnapshot[]) {
  return channels
    .filter((c) => c.kind === "message" || c.bridge?.tap?.recordMessages === true)
    .map((c) => ({ id: c.id, label: c.label }));
}

interface Empty {
  title: string;
  description: string;
  /** What the action offers: clear the filters, or open the time range. */
  action: "clear" | "any" | null;
}

/**
 * What an empty answer means, said as it is true: nothing stored yet, filters that exclude
 * every message, or messages that exist outside the time range.
 */
function emptyFor(filters: MessageFilters, store: StoreStatus | null | undefined): Empty {
  if (isNarrowed(filters)) {
    return {
      title: "No messages match",
      description: "No stored message in the selected time range matches these filters.",
      action: "clear",
    };
  }
  if (isAnyTime(filters) || store?.messageCount === 0) {
    return {
      title: "No messages yet",
      description: "Messages appear here as the message channels receive them.",
      action: null,
    };
  }
  return {
    title: "No messages in this time range",
    description: "Messages are stored, but none was received in the selected time range.",
    action: "any",
  };
}

/**
 * Messages (CON-04, spec §6): the engine filters, sorts and pages, so only one page is ever
 * read, however many messages are stored. The filters but the identifier, text and message ID,
 * the sort and the page live in the URL; those three, the rows per page and the chosen columns
 * are kept in memory while the console is open (`messagesSession`), so a message opened from
 * the list and the way back find the working set as it was. The rows of the previous answer
 * stay on screen while the next one loads. Export… saves every message the filters select, not
 * only the page. When the store cannot be read the page says so; that is not an error.
 */
export function MessagesPage() {
  const [view, setView] = useMessageView();
  // Rows per page is the console session's choice, not the address's: a link opens with it.
  const [pageSize, setPageSizeState] = useState(() => messagesSession().pageSize);
  // The columns chosen by hand; `null` is the default set, as many as fit.
  const [chosen, setChosenState] = useState(() => messagesSession().columns);
  const setPageSize = (size: number) => {
    keepMessagesSession({ pageSize: size });
    setPageSizeState(size);
  };
  const setChosen = (ids: readonly string[] | null) => {
    keepMessagesSession({ columns: ids });
    setChosenState(ids);
  };
  const frame = useRef<HTMLDivElement>(null);
  const frameWidth = useElementWidth(frame);
  const [resending, setResending] = useState<MessageSummary | null>(null);
  const [exporting, setExporting] = useState(false);
  const snapshot = useSnapshot();
  const store = snapshot.data?.node.store;
  const channels = useMemo(() => storedChannels(snapshot.data?.channels ?? []), [snapshot.data]);
  const query = useEngineQuery(
    ["listMessages", view, pageSize],
    (api) => api.listMessages(toMessageQuery(view, pageSize, Date.now())),
    { placeholderData: keepPreviousData },
  );
  const rowActions = useMemo<GridRowActions<MessageSummary>>(
    () => ({
      label: (message) => identifierOf(message) ?? message.id,
      hasItems: (message) => canResend(message.state),
      items: (message) => [
        { id: "resend", label: "Resend…", onAction: () => setResending(message) },
      ],
    }),
    [],
  );
  const fitting = useMemo(() => fittingColumns(frameWidth), [frameWidth]);
  const shown = chosen ?? fitting;
  const columns = useMemo(() => messageColumns(shown), [shown]);
  const problem = storeProblem(query.error, store);
  if (problem !== null) {
    return (
      <Page>
        <PageHeader title="Messages" />
        <SampleDataNotice />
        <StoreNotAvailable problem={problem} />
      </Page>
    );
  }
  const changeFilters = (filters: MessageFilters) => setView({ ...view, filters, page: 1 });
  const empty = emptyFor(view.filters, store);
  const emptyAction =
    empty.action === "clear" ? (
      <Button onPress={() => changeFilters(DEFAULT_FILTERS)}>Clear filters</Button>
    ) : empty.action === "any" ? (
      <Button onPress={() => changeFilters({ ...view.filters, from: undefined, to: undefined })}>
        Show any time
      </Button>
    ) : undefined;
  return (
    <Page layout="collection">
      <PageHeader title="Messages" />
      <SampleDataNotice />
      <MessageFilterBar
        value={view.filters}
        onChange={changeFilters}
        channels={channels}
        actions={
          <>
            <ColumnChooser
              options={COLUMN_CHOICES}
              value={choosableIn(shown)}
              chosen={chosen}
              fitting={fitting}
              defaultCount={DEFAULT_ORDER.length}
              onChange={setChosen}
            />
            <Button onPress={() => setExporting(true)}>Export…</Button>
            <RefreshControl query={query} />
          </>
        }
      />
      <div ref={frame} className={styles.frame}>
        <QueryContent
          query={query}
          what="messages"
          loadingRows={8}
          empty={{
            // An empty answer to an earlier view is not the answer to this one.
            when: (data) => data.total === 0 && data.rows.length === 0 && !query.isPlaceholderData,
            title: empty.title,
            description: empty.description,
            action: emptyAction,
          }}
        >
          {(data) =>
            query.isPlaceholderData && data.rows.length === 0 ? (
              <LoadingState label="Loading messages…" rows={8} />
            ) : (
              <Results
                data={data}
                view={view}
                pageSize={pageSize}
                columns={columns}
                rowActions={rowActions}
                isStale={query.isPlaceholderData}
                isFetching={query.isFetching}
                onViewChange={setView}
                onPageSizeChange={(size) => {
                  setPageSize(size);
                  setView({ ...view, page: 1 });
                }}
              />
            )
          }
        </QueryContent>
      </div>
      {resending && (
        <ResendDialog subject={subjectOfSummary(resending)} onClose={() => setResending(null)} />
      )}
      {exporting && (
        <ExportDialog
          filters={filterWords(
            view.filters,
            (id) => channels.find((channel) => channel.id === id)?.label ?? id,
          )}
          total={query.data?.total ?? null}
          filter={() => toMessageFilter(view.filters, Date.now())}
          onClose={() => setExporting(false)}
        />
      )}
    </Page>
  );
}

interface ResultsProps {
  data: MessagePage;
  view: MessageView;
  pageSize: number;
  columns: ReturnType<typeof messageColumns>;
  rowActions: GridRowActions<MessageSummary>;
  /** The rows shown answer an earlier view; the answer to this one is loading. */
  isStale: boolean;
  isFetching: boolean;
  onViewChange: (view: MessageView) => void;
  onPageSizeChange: (pageSize: number) => void;
}

/** The grid of one page, and its paging bar. */
function Results({
  data,
  view,
  pageSize,
  columns,
  rowActions,
  isStale,
  isFetching,
  onViewChange,
  onPageSizeChange,
}: ResultsProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const offset = (view.page - 1) * pageSize;
  if (data.rows.length === 0) {
    return (
      <EmptyState
        title="This page is past the last message"
        description="Fewer messages match than when this page was opened."
        action={
          <Button onPress={() => onViewChange({ ...view, page: 1 })}>Go to the first page</Button>
        }
      />
    );
  }
  const sort: GridSort = { id: view.sort.field, descending: view.sort.descending };
  return (
    <>
      <DataGrid
        label="Messages"
        columns={columns}
        rows={data.rows}
        getRowId={getRowId}
        fill
        sort={sort}
        rowActions={rowActions}
        onSortChange={(next) =>
          onViewChange({
            ...view,
            sort:
              next && isSortField(next.id)
                ? { field: next.id, descending: next.descending }
                : DEFAULT_SORT,
            page: 1,
          })
        }
        onRowAction={(message) =>
          void navigate(`/messages/${encodeURIComponent(message.id)}`, {
            state: { messagesSearch: location.search } satisfies MessageListState,
          })
        }
        isLoading={isStale}
        rowWindow={{ offset, total: data.total }}
        scrollResetKey={`${offset}|${pageSize}|${view.sort.field}|${view.sort.descending}`}
      />
      <PagingBar
        label="Message pages"
        noun="Rows"
        offset={offset}
        rowCount={data.rows.length}
        total={data.total}
        totalExact
        pageSize={pageSize}
        pageSizes={PAGE_SIZES}
        isRefreshing={isFetching}
        onOffsetChange={(next) => onViewChange({ ...view, page: Math.floor(next / pageSize) + 1 })}
        onPageSizeChange={onPageSizeChange}
      />
    </>
  );
}
