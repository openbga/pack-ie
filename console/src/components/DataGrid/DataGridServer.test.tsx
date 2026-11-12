import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderWithProviders } from "@/lib/test/render";
import { DataGrid } from "./DataGrid";
import type { DataGridColumn, GridRowWindow, GridSort } from "./gridTypes";

interface Item {
  id: string;
  channel: string;
}

const COLUMNS: readonly DataGridColumn<Item>[] = [
  {
    id: "id",
    header: "Message",
    identifier: true,
    cell: (item) => item.id,
    sortable: true,
    defaultSort: "desc",
  },
  { id: "channel", header: "Channel", cell: (item) => item.channel, sortable: true, grow: 3 },
  { id: "note", header: "Note", cell: () => "–" },
];

const page = (from: number, count: number): Item[] =>
  Array.from({ length: count }, (_, index) => ({
    id: `SYN-${String(from + index).padStart(6, "0")}`,
    channel: index % 2 === 0 ? "Synthetic chemistry" : "Synthetic hematology",
  }));

const getRowId = (item: Item) => item.id;
const header = (name: string) => screen.getByRole("columnheader", { name });

interface HarnessProps {
  rows: Item[];
  initial?: GridSort | null;
  rowWindow?: GridRowWindow;
  isLoading?: boolean;
  onSortChange?: (sort: GridSort | null) => void;
  scrollResetKey?: number;
  onAtTopChange?: (atTop: boolean) => void;
}

/** A server-driven grid as a page composes it: the sort lives in the caller, rows come ready ordered. */
function Harness({ rows, initial = null, onSortChange, ...rest }: HarnessProps) {
  const [sort, setSort] = useState<GridSort | null>(initial);
  return (
    <DataGrid
      label="Synthetic messages"
      columns={COLUMNS}
      rows={rows}
      getRowId={getRowId}
      sort={sort}
      onSortChange={(next) => {
        setSort(next);
        onSortChange?.(next);
      }}
      {...rest}
    />
  );
}

describe("DataGrid driven by the engine", () => {
  beforeEach(mockGridLayout);

  it("reports the next sort instead of sorting, cycling descending-first, ascending, none", async () => {
    const onSortChange = vi.fn();
    const rows = page(1, 3);
    const { user } = renderWithProviders(<Harness rows={rows} onSortChange={onSortChange} />);
    expect(header("Message").getAttribute("aria-sort")).toBe("none");
    await user.click(header("Message"));
    expect(onSortChange).toHaveBeenLastCalledWith({ id: "id", descending: true });
    expect(header("Message").getAttribute("aria-sort")).toBe("descending");
    await user.click(header("Message"));
    expect(onSortChange).toHaveBeenLastCalledWith({ id: "id", descending: false });
    await user.click(header("Message"));
    expect(onSortChange).toHaveBeenLastCalledWith(null);
    expect(header("Message").getAttribute("aria-sort")).toBe("none");
    // Rows are the engine's: the grid never reorders them itself.
    expect(gridRows().map((row) => row.textContent?.slice(0, 10))).toEqual([
      "SYN-000001",
      "SYN-000002",
      "SYN-000003",
    ]);
  });

  it("starts a column ascending when it has no descending default, and sorts on Enter", async () => {
    const onSortChange = vi.fn();
    const { user } = renderWithProviders(
      <Harness
        rows={page(1, 2)}
        initial={{ id: "id", descending: true }}
        onSortChange={onSortChange}
      />,
    );
    expect(header("Message").getAttribute("aria-sort")).toBe("descending");
    await user.click(header("Channel"));
    expect(onSortChange).toHaveBeenLastCalledWith({ id: "channel", descending: false });
    expect(header("Message").getAttribute("aria-sort")).toBe("none");
    await user.keyboard("{Enter}");
    expect(onSortChange).toHaveBeenLastCalledWith({ id: "channel", descending: true });
    // A column the engine cannot sort offers nothing.
    expect(header("Note").hasAttribute("aria-sort")).toBe(false);
  });

  it("replaces its rows when a new page arrives, keeping the old ones and marking busy while it loads", () => {
    const { rerender } = render(<Harness rows={page(1, 3)} />);
    const grid = screen.getByRole("grid", { name: "Synthetic messages" });
    expect(grid.hasAttribute("aria-busy")).toBe(false);
    rerender(<Harness rows={page(1, 3)} isLoading />);
    expect(grid.getAttribute("aria-busy")).toBe("true");
    expect(gridRows()).toHaveLength(3);
    rerender(<Harness rows={page(101, 2)} />);
    expect(grid.hasAttribute("aria-busy")).toBe(false);
    expect(gridRows().map((row) => row.textContent?.slice(0, 10))).toEqual([
      "SYN-000101",
      "SYN-000102",
    ]);
  });

  it("counts rows of the whole answer for assistive technology, not just the page", async () => {
    const { container, rerender } = render(
      <Harness rows={page(101, 100)} rowWindow={{ offset: 100, total: 412_455 }} />,
    );
    const grid = screen.getByRole("grid");
    expect(grid.getAttribute("aria-rowcount")).toBe("412456");
    expect(gridRows()[0]?.getAttribute("aria-rowindex")).toBe("102");
    expect(screen.getAllByRole("row")[0]?.getAttribute("aria-rowindex")).toBe("1");
    await expectNoA11yViolations(container);
    rerender(<Harness rows={page(1, 3)} rowWindow={{ offset: 0, total: null }} />);
    expect(grid.getAttribute("aria-rowcount")).toBe("-1");
  });

  it("scrolls back to the first row when its reset key changes", () => {
    const { rerender } = render(<Harness rows={page(1, 150)} scrollResetKey={1} />);
    const grid = screen.getByRole("grid");
    act(() => {
      grid.scrollTop = 400;
    });
    rerender(<Harness rows={page(1, 150)} scrollResetKey={1} />);
    expect(grid.scrollTop).toBe(400);
    rerender(<Harness rows={page(151, 150)} scrollResetKey={2} />);
    expect(grid.scrollTop).toBe(0);
  });

  it("reports leaving and returning to the first row, once per change", () => {
    const onAtTopChange = vi.fn();
    renderWithProviders(<Harness rows={page(1, 150)} onAtTopChange={onAtTopChange} />);
    const grid = screen.getByRole("grid");
    const scrollTo = (top: number) => {
      grid.scrollTop = top;
      fireEvent.scroll(grid);
    };
    scrollTo(0);
    expect(onAtTopChange).not.toHaveBeenCalled();
    scrollTo(300);
    scrollTo(600);
    expect(onAtTopChange.mock.calls).toEqual([[false]]);
    scrollTo(0);
    expect(onAtTopChange.mock.calls).toEqual([[false], [true]]);
  });

  it("gives a column more of the remaining width with grow", () => {
    renderWithProviders(<Harness rows={page(1, 1)} />);
    const template = (gridRows()[0] as HTMLElement).style.gridTemplateColumns;
    expect(template).toContain("minmax(12rem, 1fr)");
    expect(template).toContain("minmax(8rem, 3fr)");
  });
});
