import { useMemo, useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { AuditEvent } from "@/api/types";
import { Button } from "@/components/Button";
import { ColumnChooser } from "@/components/ChecklistPopover";
import { DataGrid } from "@/components/DataGrid";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { NoResults } from "@/components/FindField";
import { ListFooter, ListHint, ListLayout } from "@/components/ListLayout";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { RefreshControl } from "@/components/RefreshControl";
import { formatCount } from "@/lib/format";
import { useElementWidth } from "@/lib/useElementWidth";
import { AuditEventLoading, AuditEventMissing, AuditEventPanel } from "./AuditEventPanel";
import { AuditFilterBar } from "./AuditFilterBar";
import { auditColumns, COLUMN_CHOICES, choosableIn, DEFAULT_COLUMN_COUNT } from "./auditColumns";
import { DEFAULT_VIEW, describeView, isFiltered, toAuditFilter } from "./auditParams";
import { fittingColumns } from "./columnFit";
import { ExportDialog } from "./ExportDialog";
import { useAuditEvents, useOpenedEvent } from "./useAuditEvents";
import { useAuditView } from "./useAuditView";
import { VerifyResult } from "./VerifyResult";

const getRowId = (event: AuditEvent) => String(event.seq);
const NONE: readonly AuditEvent[] = [];

/**
 * Audit (docs/spec/audit.md): who did what through the engine's management interface, newest
 * first, a page at a time. Filters and the open event are in the URL and run on the engine.
 * Exporting and verifying the chain are recorded in the log; reading it is not.
 */
export function AuditPage() {
  const { view, setView, seq, setSeq } = useAuditView();
  const { query, events } = useAuditEvents(view);
  const rows = events ?? NONE;
  const opened = useOpenedEvent(seq, events);
  const [exporting, setExporting] = useState(false);
  const verify = useEngineMutation((api) => api.verifyAudit(), { invalidates: ["getAuditEvents"] });
  const filtered = isFiltered(view);
  // The list's width, which the open event takes part of, decides how many columns fit. The list
  // mounts once the events have loaded, so it is tracked as state for the measuring to follow it.
  const [list, setList] = useState<HTMLDivElement | null>(null);
  const listWidth = useElementWidth(useMemo(() => ({ current: list }), [list]));
  // The columns chosen by hand; `null` is the default set, as many as fit.
  const [chosen, setChosen] = useState<readonly string[] | null>(null);
  const fitting = useMemo(() => fittingColumns(listWidth), [listWidth]);
  const shown = chosen ?? fitting;
  const columns = useMemo(() => auditColumns(shown), [shown]);

  return (
    <Page layout="collection">
      <PageHeader
        title="Audit"
        description="Who did what through the engine's management interface, newest first. Actor and object ID match exactly."
      />
      <AuditFilterBar
        value={view}
        onChange={setView}
        count={
          events !== undefined && (
            <Count
              loaded={rows.length}
              more={query.hasNextPage}
              updating={query.isPlaceholderData}
            />
          )
        }
        actions={
          <>
            <ColumnChooser
              options={COLUMN_CHOICES}
              value={choosableIn(shown)}
              chosen={chosen}
              fitting={fitting}
              defaultCount={DEFAULT_COLUMN_COUNT}
              onChange={setChosen}
            />
            <Button onPress={() => setExporting(true)}>Export…</Button>
            <Button isPending={verify.isPending} onPress={() => verify.mutate()}>
              Verify chain
            </Button>
            <RefreshControl query={query} />
          </>
        }
      />
      {verify.data && <VerifyResult result={verify.data} onDismiss={() => verify.reset()} />}
      {verify.error && (
        <ErrorState
          title="The chain could not be verified"
          error={verify.error}
          onRetry={() => verify.mutate()}
          isRetrying={verify.isPending}
        />
      )}
      {query.isLoadingError && query.error.code === "unavailable" ? (
        <ErrorState
          title="The audit log cannot be read. Audited actions are refused until it can be written again."
          error={query.error}
          onRetry={() => void query.refetch()}
          isRetrying={query.isFetching}
        />
      ) : (
        <QueryContent query={query} what="audit events" loadingRows={8}>
          {() => (
            <ListLayout
              listRef={setList}
              panel={
                seq !== null && (
                  <Detail
                    seq={seq}
                    rows={rows}
                    opened={opened}
                    onSelect={setSeq}
                    onClose={() => setSeq(null)}
                  />
                )
              }
            >
              {rows.length === 0 ? (
                <Empty
                  filtered={filtered}
                  exact={view.actor !== undefined || view.objectId !== undefined}
                  onClear={() => setView(DEFAULT_VIEW)}
                />
              ) : (
                <>
                  <DataGrid
                    label="Audit events"
                    columns={columns}
                    rows={rows}
                    getRowId={getRowId}
                    fill
                    isLoading={query.isPlaceholderData}
                    onRowAction={(event) => setSeq(event.seq)}
                    openRowId={seq === null ? null : String(seq)}
                  />
                  <ListFooter>
                    {!query.isPlaceholderData && query.hasNextPage && (
                      <Button
                        isPending={query.isFetchingNextPage}
                        onPress={() => void query.fetchNextPage()}
                      >
                        Load older events
                      </Button>
                    )}
                    {!query.isPlaceholderData && !query.hasNextPage && (
                      <span>No older events.</span>
                    )}
                  </ListFooter>
                </>
              )}
            </ListLayout>
          )}
        </QueryContent>
      )}
      {exporting && (
        <ExportDialog
          filters={describeView(view)}
          filter={() => toAuditFilter(view, Date.now())}
          onClose={() => setExporting(false)}
        />
      )}
    </Page>
  );
}

/** What the list holds, said honestly: events loaded, and whether older ones remain. */
function Count({ loaded, more, updating }: { loaded: number; more: boolean; updating: boolean }) {
  // The rows of the previous filters stay until the new answer arrives; they are not counted.
  if (updating) return "Updating…";
  const events = `${formatCount(loaded)} ${loaded === 1 ? "event" : "events"} loaded`;
  return more ? `${events} · older events available` : `${events} · no older events`;
}

function Empty({
  filtered,
  exact,
  onClear,
}: {
  filtered: boolean;
  exact: boolean;
  onClear: () => void;
}) {
  if (!filtered) {
    return (
      <EmptyState
        title="No audit events yet"
        description="Events appear here as people use the console and the command line, and when the engine starts and stops."
      />
    );
  }
  return (
    <>
      <NoResults noun="audit events" query="" onClear={onClear} />
      {exact && <ListHint>Actor and object ID must match exactly, including case.</ListHint>}
    </>
  );
}

interface DetailProps {
  seq: number;
  rows: readonly AuditEvent[];
  opened: ReturnType<typeof useOpenedEvent>;
  onSelect: (seq: number) => void;
  onClose: () => void;
}

/** The open event: from the list, or read on its own when a link points beyond what is loaded. */
function Detail({ seq, rows, opened, onSelect, onClose }: DetailProps) {
  const index = rows.findIndex((event) => event.seq === seq);
  const event = rows[index] ?? opened.data;
  if (event) {
    const newer = index > 0 ? rows[index - 1] : undefined;
    const older = index >= 0 ? rows[index + 1] : undefined;
    return (
      <AuditEventPanel
        event={event}
        onClose={onClose}
        onPrevious={newer && (() => onSelect(newer.seq))}
        onNext={older && (() => onSelect(older.seq))}
      />
    );
  }
  if (opened.isPending) return <AuditEventLoading seq={seq} onClose={onClose} />;
  return (
    <AuditEventMissing
      seq={seq}
      error={opened.error}
      onRetry={() => void opened.refetch()}
      onClose={onClose}
    />
  );
}
