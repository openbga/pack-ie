import type { UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { EngineError } from "@/api/types";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { PageHeader } from "@/components/Page";

interface QueryContentProps<Data> {
  query: UseQueryResult<Data, EngineError>;
  /** What is loading, in lower case, such as "runs". */
  what: string;
  /**
   * Shown instead of `children` when the answer has nothing to show; `action` is the next
   * step, such as "Show all" when a filter excludes every record.
   */
  empty?: { when: (data: Data) => boolean; title: string; description: string; action?: ReactNode };
  /**
   * For a page whose `PageHeader` comes from the record: a generic title, such as "Report",
   * shown as the page's `h1` while the record loads or when it cannot be loaded.
   */
  pageTitle?: string;
  /** Placeholder rows while loading, where a grid will appear. */
  loadingRows?: number;
  children: (data: Data) => ReactNode;
}

/**
 * An engine read with its loading, error (with retry) and empty states. When a refresh
 * fails, the data already shown stays, under a "Could not refresh" message.
 */
export function QueryContent<Data>({
  query,
  what,
  empty,
  pageTitle,
  loadingRows,
  children,
}: QueryContentProps<Data>) {
  const header = pageTitle === undefined ? null : <PageHeader title={pageTitle} />;
  const retry = () => void query.refetch();
  if (query.isPending) {
    return (
      <>
        {header}
        <LoadingState label={`Loading ${what}…`} rows={loadingRows} />
      </>
    );
  }
  if (query.isLoadingError) {
    return (
      <>
        {header}
        <ErrorState
          title={`The ${what} could not be loaded`}
          error={query.error}
          onRetry={retry}
          isRetrying={query.isFetching}
        />
      </>
    );
  }
  const fetchNextPage =
    "fetchNextPage" in query && typeof query.fetchNextPage === "function"
      ? query.fetchNextPage
      : undefined;
  const pageError = "isFetchNextPageError" in query &&
    query.isFetchNextPageError === true &&
    query.error && (
      <ErrorState
        title={`More ${what} could not be loaded. The loaded rows remain available.`}
        error={query.error}
        onRetry={() => void fetchNextPage?.()}
      />
    );
  const refreshError = query.isRefetchError && (
    <ErrorState
      title={`Could not refresh. The ${what} shown may be out of date.`}
      error={query.error}
      onRetry={retry}
    />
  );
  return (
    <>
      {refreshError}
      {pageError}
      {empty?.when(query.data) ? (
        <EmptyState title={empty.title} description={empty.description} action={empty.action} />
      ) : (
        children(query.data)
      )}
    </>
  );
}
