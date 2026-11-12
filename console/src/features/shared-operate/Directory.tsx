import type { UseQueryResult } from "@tanstack/react-query";
import { type ReactNode, useMemo, useRef } from "react";
import type { EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { DataGrid, type DataGridColumn, type GridRowActions } from "@/components/DataGrid";
import { FindCount, FindField, findRows, NoResults } from "@/components/FindField";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { Toolbar } from "@/components/Toolbar";
import { useElementWidth } from "@/lib/useElementWidth";
import styles from "./Directory.module.css";
import { useFindParam } from "./searchParam";

/**
 * The columns a grid of `width` pixels shows (null while the width is not measured, as where
 * there is no layout); a function when columns drop out as the window narrows.
 */
type DirectoryColumns<Row extends object> =
  | readonly DataGridColumn<Row>[]
  | ((width: number | null) => readonly DataGridColumn<Row>[]);

interface DirectoryProps<Data, Row extends object> {
  /** The page's `h1`, equal to its navigation label, and the grid's name. */
  title: string;
  /** What the directory holds, lower case. */
  noun: { many: string; one: string };
  /** "Add …", in the header. */
  headerAction?: ReactNode;
  query: UseQueryResult<Data, EngineError>;
  /** The whole collection in the answer; a module-level function, so it stays stable. */
  rowsOf: (data: Data) => readonly Row[];
  /** The compact filter controls, after Find. */
  filters?: ReactNode;
  /** Whether any filter is set, which offers "Clear filters". */
  isFiltered: boolean;
  onClearFilters: () => void;
  /** Keeps the rows the filters allow; stable while the filters do not change (`useMemo`). */
  keep?: (row: Row) => boolean;
  /** The text Find searches; omit for a list without Find. A module-level function. */
  find?: { placeholder: string; fields: (row: Row) => readonly (string | null)[] };
  /** After the filters, before the count: refresh and other list actions. */
  toolbarActions?: ReactNode;
  /** Shown when the collection has nothing in it at all. */
  empty: { title: string; description: string; action?: ReactNode };
  columns: DirectoryColumns<Row>;
  getRowId: (row: Row) => string;
  onRowAction?: (row: Row, order: readonly string[]) => void;
  rowActions?: GridRowActions<Row>;
  /** Dialogs and other parts rendered once beside the list. */
  children?: ReactNode;
}

/**
 * A directory of the Operate screens (collection recipe of the page layout standard): the
 * title with its Add action, a compact toolbar of Find, filters and one count, and one grid
 * filling the page whose rows open their record. "Nothing yet" and "nothing matches" are
 * different states with different next steps. The engine returns each whole collection, so
 * the filters and Find run here, and the count says "12 of 28 channels".
 */
export function Directory<Data, Row extends object>(props: DirectoryProps<Data, Row>) {
  const { title, noun, query } = props;
  const [findText, setFindText] = useFindParam();
  const { rowsOf, keep, find } = props;
  const all = useMemo(
    () => (query.data === undefined ? [] : rowsOf(query.data)),
    [query.data, rowsOf],
  );
  const shown = useMemo(() => {
    const kept = keep ? all.filter(keep) : all;
    return find ? findRows(kept, findText, find.fields) : kept;
  }, [all, keep, find, findText]);
  const frame = useRef<HTMLDivElement>(null);
  const width = useElementWidth(frame);
  const { columns } = props;
  const shownColumns = useMemo(
    () => (typeof columns === "function" ? columns(width) : columns),
    [columns, width],
  );
  const clear = () => (findText.trim() === "" ? props.onClearFilters() : setFindText(""));
  return (
    <Page layout="collection">
      <PageHeader title={title} actions={props.headerAction} />
      <Toolbar
        variant="collection"
        actions={
          <>
            {props.isFiltered && (
              <Button variant="quiet" onPress={props.onClearFilters}>
                Clear filters
              </Button>
            )}
            {props.toolbarActions}
          </>
        }
        count={
          query.data !== undefined && (
            <FindCount shown={shown.length} total={all.length} noun={noun.many} one={noun.one} />
          )
        }
      >
        {find && (
          <FindField
            label={`Find ${noun.many}`}
            placeholder={find.placeholder}
            value={findText}
            onChange={setFindText}
          />
        )}
        {props.filters}
      </Toolbar>
      <div ref={frame} className={styles.frame}>
        <QueryContent
          query={query}
          what={noun.many}
          loadingRows={8}
          empty={{
            when: () => all.length === 0,
            title: props.empty.title,
            description: props.empty.description,
            action: props.empty.action,
          }}
        >
          {() =>
            shown.length === 0 ? (
              <NoResults noun={noun.many} query={findText} onClear={clear} />
            ) : (
              <DataGrid
                label={title}
                columns={shownColumns}
                rows={shown}
                getRowId={props.getRowId}
                onRowAction={props.onRowAction}
                rowActions={props.rowActions}
                fill
              />
            )
          }
        </QueryContent>
      </div>
      {props.children}
    </Page>
  );
}
