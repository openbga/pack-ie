import { useMemo, useRef, useState } from "react";
import { useConnection, useSnapshot } from "@/api/hooks";
import { Button } from "@/components/Button";
import { DataGrid } from "@/components/DataGrid";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { NoResults } from "@/components/FindField";
import { ListFooter, ListHint, ListLayout } from "@/components/ListLayout";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { RefreshControl } from "@/components/RefreshControl";
import { formatCount, useFormatTime } from "@/lib/format";
import { FollowControls } from "./FollowControls";
import { LogEventPanel } from "./LogEventPanel";
import { LogFilterBar } from "./LogFilterBar";
import { type LogLabels, useLogColumns } from "./logColumns";
import { isKnownLogEvent } from "./logEvents";
import { canFollow, DEFAULT_FILTERS, isFiltered, type LogFilters } from "./logParams";
import type { EventRow, LogRow } from "./logRows";
import { useLogFilters } from "./useLogFilters";
import { MAX_EVENTS, useLogStream } from "./useLogStream";
import { useStableOptions } from "./useStableOptions";

const getRowId = (row: LogRow) => row.key;

/**
 * Logs (CON-05): the engine's structured log, newest first. Following adds new events at the top
 * and holds still while the operator scrolls away or opens an event; older events load on
 * request. Filters are in the URL and run on the engine; a bounded window is kept in memory.
 * Following survives the engine going away: while it is, the page waits, and what the outage
 * cost is a divider row in the list.
 */
export function LogsPage() {
  const [filters, setFilters] = useLogFilters();
  const snapshot = useSnapshot();
  const channels = useStableOptions(
    (snapshot.data?.channels ?? []).map(({ id, label }) => ({ id, label })),
  );
  const endpoints = useStableOptions(
    (snapshot.data?.endpoints ?? []).map(({ id, label }) => ({ id, label })),
  );
  const labels = useMemo<LogLabels>(
    () => ({
      channels: new Map(channels.map((option) => [option.id, option.label])),
      endpoints: new Map(endpoints.map((option) => [option.id, option.label])),
    }),
    [channels, endpoints],
  );
  // Following unless the operator chose otherwise, which holds until the time range changes.
  const [followChoice, setFollowChoice] = useState<boolean | null>(null);
  const following = canFollow(filters) && (followChoice ?? filters.range === "live");
  const connection = useConnection();
  const waiting =
    following && connection.data !== undefined && connection.data.state !== "connected";
  const stream = useLogStream(filters, following);
  const [selected, setSelected] = useState<EventRow | null>(null);
  const atTop = useRef(true);
  const [scrollKey, setScrollKey] = useState(0);

  function changeFilters(next: LogFilters) {
    if (next.range !== filters.range) setFollowChoice(null);
    atTop.current = true;
    setSelected(null);
    setFilters(next);
  }

  function select(row: EventRow | null) {
    setSelected(row);
    if (row === null) stream.resume("selection", atTop.current);
    else if (following) stream.pause("selection");
  }

  /** Events before the oldest one shown: a time range ending there, which does not follow. */
  function queryBefore(at: string) {
    atTop.current = true;
    setSelected(null);
    setFilters({ ...withoutTimes(filters), range: "custom", to: at });
  }

  return (
    <Page layout="collection">
      <PageHeader title="Logs" />
      <LogFilterBar
        value={filters}
        onChange={changeFilters}
        channels={channels}
        endpoints={endpoints}
        count={<Count stream={stream} />}
        actions={
          <>
            <FollowControls
              following={following}
              unavailable={!canFollow(filters)}
              onFollowChange={(on) => {
                setFollowChoice(on);
                // Events logged while not following are not in the list: read it again.
                if (on) void stream.refresh();
              }}
              waiting={waiting}
              paused={stream.paused}
              held={stream.held}
              onPause={() => stream.pause("manual")}
              onResume={() => {
                atTop.current = true;
                setScrollKey((key) => key + 1);
                stream.resume("manual", true);
              }}
            />
            <RefreshControl
              query={{
                dataUpdatedAt: stream.base.dataUpdatedAt,
                isFetching: stream.base.isFetching,
                refetch: stream.refresh,
              }}
            />
          </>
        }
      />
      <QueryContent query={stream.base} what="log events" loadingRows={8}>
        {() =>
          stream.rows.length === 0 ? (
            <Empty filters={filters} onClear={() => changeFilters(DEFAULT_FILTERS)} />
          ) : (
            <ListLayout
              panel={
                selected && (
                  <LogEventPanel
                    event={selected.event}
                    labels={labels}
                    onClose={() => select(null)}
                    {...neighbours(stream.rows, selected, select)}
                  />
                )
              }
            >
              <Events
                stream={stream}
                labels={labels}
                selected={selected}
                scrollKey={scrollKey}
                onSelect={select}
                onQueryBefore={queryBefore}
                onAtTopChange={(top) => {
                  atTop.current = top;
                  if (!following) return;
                  if (top) stream.resume("scroll", true);
                  else stream.pause("scroll");
                }}
              />
            </ListLayout>
          )
        }
      </QueryContent>
    </Page>
  );
}

type Stream = ReturnType<typeof useLogStream>;

/** The filters without a time range, such as "Last hour". */
function withoutTimes(filters: LogFilters): LogFilters {
  const { from: _from, to: _to, ...rest } = filters;
  return rest;
}

/** What the list holds, said honestly: loaded events, and whether older ones remain. */
function Count({ stream }: { stream: Stream }) {
  if (stream.base.data === undefined) return null;
  const loaded = `${formatCount(stream.count)} loaded`;
  if (stream.windowFull) return `${loaded} · window full`;
  return stream.atEnd
    ? `${loaded} · no older events in the engine's log`
    : `${loaded} · older events available`;
}

/** Previous (newer) and Next (older) events of the list, skipping dividers, for the panel. */
function neighbours(rows: readonly LogRow[], selected: EventRow, select: (row: EventRow) => void) {
  const index = rows.findIndex((row) => row.key === selected.key);
  const events = (candidates: readonly LogRow[]) =>
    candidates.find((row): row is EventRow => row.kind === "event");
  const newer = index > 0 ? events(rows.slice(0, index).reverse()) : undefined;
  const older = index >= 0 ? events(rows.slice(index + 1)) : undefined;
  return {
    onPrevious: newer && (() => select(newer)),
    onNext: older && (() => select(older)),
  };
}

function Empty({ filters, onClear }: { filters: LogFilters; onClear: () => void }) {
  if (isFiltered(filters)) {
    return (
      <>
        <NoResults noun="events" query={filters.text ?? ""} onClear={onClear} />
        {filters.event !== undefined && !isKnownLogEvent(filters.event) && (
          <ListHint>The engine matches the whole event name, such as channel.started.</ListHint>
        )}
      </>
    );
  }
  return (
    <EmptyState
      title="No log events yet"
      description="Events appear here as the engine logs them."
    />
  );
}

interface EventsProps {
  stream: Stream;
  labels: LogLabels;
  selected: EventRow | null;
  scrollKey: number;
  onSelect: (row: EventRow) => void;
  onQueryBefore: (at: string) => void;
  onAtTopChange: (atTop: boolean) => void;
}

/** The grid of events, and below it how to reach older ones. */
function Events({
  stream,
  labels,
  selected,
  scrollKey,
  onSelect,
  onQueryBefore,
  onAtTopChange,
}: EventsProps) {
  const columns = useLogColumns(labels);
  return (
    <>
      <DataGrid
        label="Log events"
        columns={columns}
        rows={stream.rows}
        getRowId={getRowId}
        fill
        onRowAction={(row) => {
          if (row.kind === "event") onSelect(row);
        }}
        openRowId={selected ? getRowId(selected) : null}
        scrollResetKey={scrollKey}
        onAtTopChange={onAtTopChange}
      />
      <ListFooter>
        {stream.olderError && (
          <ErrorState title="Older events could not be loaded" error={stream.olderError} />
        )}
        {stream.canLoadOlder && (
          <Button isPending={stream.loadingOlder} onPress={() => void stream.loadOlder()}>
            Load older events
          </Button>
        )}
        <OlderNote stream={stream} onQueryBefore={onQueryBefore} />
      </ListFooter>
    </>
  );
}

/** The oldest event in the list, where a query for older ones starts. */
function oldestEvent(rows: readonly LogRow[]) {
  for (let index = rows.length - 1; index >= 0; index--) {
    const row = rows[index];
    if (row?.kind === "event") return row.event;
  }
  return undefined;
}

function OlderNote({
  stream,
  onQueryBefore,
}: {
  stream: Stream;
  onQueryBefore: (at: string) => void;
}) {
  const formatTime = useFormatTime();
  if (stream.windowFull) {
    const oldest = oldestEvent(stream.rows);
    return (
      <>
        <span>
          The newest {formatCount(MAX_EVENTS)} rows are kept. Older events: choose a time range.
        </span>
        {oldest && (
          <Button onPress={() => onQueryBefore(oldest.at)}>
            Show events before {formatTime(oldest.at, { timeOnly: true, seconds: true })}
          </Button>
        )}
      </>
    );
  }
  if (stream.restarted) {
    return <span>The engine restarted: its earlier events are no longer available.</span>;
  }
  if (stream.atEnd) return <span>No older events in the engine's log.</span>;
  return null;
}
