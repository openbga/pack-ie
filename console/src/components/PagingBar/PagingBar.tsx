import { ChevronLeft, ChevronRight, ChevronsLeft } from "lucide-react";
import { Button } from "@/components/Button";
import { Select } from "@/components/Select";
import { formatCount } from "@/lib/format";
import styles from "./PagingBar.module.css";

interface PagingBarProps {
  /** Zero-based position of the first row on this page in the whole answer. */
  offset: number;
  /** Rows on this page. */
  rowCount: number;
  /** Rows in the whole answer, as far as the engine counted them. */
  total: number;
  /** False when the engine stopped counting at its cap and `total` is a lower bound. */
  totalExact: boolean;
  pageSize: number;
  /** The page sizes offered; keep `pageSize` among them. */
  pageSizes: readonly number[];
  /** What the rows are, plural and capitalised, as the count begins: "Rows 1–100 of 412". */
  noun?: string;
  /** A page was asked for: First, Previous and Next call it with the new first row's offset. */
  onOffsetChange: (offset: number) => void;
  onPageSizeChange: (pageSize: number) => void;
  /** A newer answer is loading; the rows shown stay until it arrives. */
  isRefreshing?: boolean;
  /** Names the navigation landmark; default "Pages". */
  label?: string;
}

/**
 * What the answer proves about its size: the range shown against the engine's count, which
 * is a lower bound ("or more") when the engine stopped counting.
 */
function rangeText(props: PagingBarProps): string {
  const { offset, rowCount, total, totalExact } = props;
  const noun = props.noun ?? "Rows";
  if (rowCount === 0) return `No ${noun.toLowerCase()} on this page`;
  const range = `${formatCount(offset + 1)}–${formatCount(offset + rowCount)}`;
  return `${noun} ${range} of ${formatCount(total)}${totalExact ? "" : " or more"}`;
}

/**
 * Whether a page follows. A full page might be followed by one; when the count is a lower
 * bound only a short page proves the end, otherwise the count does.
 */
function hasNext({ offset, rowCount, total, totalExact, pageSize }: PagingBarProps): boolean {
  if (rowCount < pageSize) return false;
  return totalExact ? offset + rowCount < total : true;
}

/**
 * Paging for a list the engine pages: First, Previous and Next, the range shown against the
 * count ("Rows 1–100 of 412,455", "of 1,000 or more" when the count is capped) and a page size
 * choice. There is no Last: a capped count cannot say where it is. The text is a polite live
 * region, so a new page is announced without moving focus.
 */
export function PagingBar(props: PagingBarProps) {
  const { offset, pageSize, pageSizes, onOffsetChange, onPageSizeChange } = props;
  const first = offset <= 0;
  return (
    <nav aria-label={props.label ?? "Pages"} className={styles.bar}>
      <div className={styles.buttons}>
        <Button isDisabled={first} onPress={() => onOffsetChange(0)}>
          <ChevronsLeft aria-hidden="true" />
          First
        </Button>
        <Button isDisabled={first} onPress={() => onOffsetChange(Math.max(0, offset - pageSize))}>
          <ChevronLeft aria-hidden="true" />
          Previous
        </Button>
        <Button isDisabled={!hasNext(props)} onPress={() => onOffsetChange(offset + pageSize)}>
          Next
          <ChevronRight aria-hidden="true" />
        </Button>
      </div>
      <p role="status" className={styles.range}>
        {rangeText(props)}
        {props.isRefreshing && <span className={styles.updating}> · Updating…</span>}
      </p>
      <div className={styles.size}>
        <Select
          layout="inline"
          label="Rows per page"
          name="pageSize"
          options={pageSizes.map((size) => ({ id: String(size), label: String(size) }))}
          value={String(pageSize)}
          onChange={(next) => {
            const size = Number(next);
            if (next !== null && size !== pageSize) onPageSizeChange(size);
          }}
        />
      </div>
    </nav>
  );
}
