import { screen, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { mockGridLayout } from "@/lib/test/layout";
import { renderWithProviders } from "@/lib/test/render";
import { DataGrid } from "./DataGrid";
import type { DataGridColumn } from "./gridTypes";

interface Item {
  id: string;
  summary: string;
  count: number;
}

const LONG = "Synthetic summary text long enough to need more than one line in a narrow column";

const items: Item[] = [
  { id: "SYN-000001", summary: LONG, count: 0 },
  { id: "SYN-000002", summary: "Short", count: 12 },
  { id: "SYN-000003", summary: "Short", count: 3 },
];

const getRowId = (item: Item) => item.id;

function columns(onCell = () => undefined): readonly DataGridColumn<Item>[] {
  return [
    {
      id: "id",
      header: "Accession",
      cell: (item) => {
        onCell();
        return item.id;
      },
    },
    { id: "summary", header: "Summary", cell: (item) => item.summary, wrap: true },
    { id: "count", header: "Results", cell: (item) => item.count, align: "end", width: 96 },
  ];
}

const COLUMNS = columns();

function Harness({ onRowAction }: { onRowAction?: (item: Item) => void }) {
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  return (
    <DataGrid
      label="Synthetic worklist"
      columns={COLUMNS}
      rows={items}
      getRowId={getRowId}
      bulk={{
        selectedIds,
        onChange: setSelectedIds,
        actions: <button type="button">Hand off</button>,
      }}
      onRowAction={onRowAction}
    />
  );
}

const cellOf = (name: string) => screen.getByRole("gridcell", { name });
const rowOf = (name: string) => cellOf(name).closest<HTMLElement>("[role=row]");

describe("DataGrid options", () => {
  beforeEach(mockGridLayout);

  it("opens a row with a single click or Enter, once for a double-click, never by arrow keys", async () => {
    const onRowAction = vi.fn();
    const { user, container } = renderWithProviders(<Harness onRowAction={onRowAction} />);
    await user.tab();
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(onRowAction).not.toHaveBeenCalled();
    await user.keyboard("{Enter}");
    expect(onRowAction).toHaveBeenLastCalledWith(items[1], [
      "SYN-000001",
      "SYN-000002",
      "SYN-000003",
    ]);
    await user.click(cellOf("12"));
    expect(onRowAction).toHaveBeenCalledTimes(2);
    await user.dblClick(cellOf("SYN-000003"));
    expect(onRowAction).toHaveBeenLastCalledWith(items[2], expect.any(Array));
    expect(onRowAction).toHaveBeenCalledTimes(3);
    await expectNoA11yViolations(container);
  });

  it("keeps Space for selection and does not open a row from its check cell", async () => {
    const onRowAction = vi.fn();
    const { user } = renderWithProviders(<Harness onRowAction={onRowAction} />);
    const row = rowOf("SYN-000001");
    if (!row) throw new Error("no row");
    const [check] = within(row).getAllByRole("gridcell");
    if (!check) throw new Error("no check cell");
    await user.dblClick(check);
    await user.keyboard(" ");
    expect(row.getAttribute("aria-selected")).toBe("true");
    expect(onRowAction).not.toHaveBeenCalled();
  });

  it("does nothing on Enter in a data row without a row action", async () => {
    const { user } = renderWithProviders(<Harness />);
    await user.tab();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(rowOf("SYN-000001")?.getAttribute("aria-selected")).toBe("false");
  });

  it("shows truncated text in full on hover, lets wrapping columns wrap, and aligns counts to the end", async () => {
    const { user } = renderWithProviders(<Harness />);
    const cut = cellOf("SYN-000001");
    vi.spyOn(cut, "scrollWidth", "get").mockReturnValue(120);
    vi.spyOn(cut, "clientWidth", "get").mockReturnValue(80);
    await user.hover(cut);
    expect(cut.getAttribute("title")).toBe("SYN-000001");
    // Text that fits, or wraps, needs no tooltip.
    await user.hover(cellOf(LONG));
    expect(cellOf(LONG).hasAttribute("title")).toBe(false);
    expect(cellOf(LONG).getAttribute("data-wrap")).toBe("true");
    expect(cellOf("12").getAttribute("data-align")).toBe("end");
    expect(screen.getByRole("columnheader", { name: "Results" }).getAttribute("data-align")).toBe(
      "end",
    );
  });

  it("shows a zero count as 0, marked to be muted", () => {
    renderWithProviders(<Harness />);
    expect(cellOf("0").hasAttribute("data-zero")).toBe(true);
    expect(cellOf("12").hasAttribute("data-zero")).toBe(false);
  });

  it("re-renders only the rows whose focus changed", async () => {
    const onCell = vi.fn();
    const counted = columns(onCell);
    const { user } = renderWithProviders(
      <DataGrid label="Synthetic worklist" columns={counted} rows={items} getRowId={getRowId} />,
    );
    await user.click(cellOf("SYN-000001"));
    onCell.mockClear();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement?.textContent).toBe("SYN-000002");
    // The row that lost the tab stop and the row that gained it; not the third row.
    expect(onCell).toHaveBeenCalledTimes(2);
  });

  it("fills its parent's height when asked", () => {
    renderWithProviders(
      <DataGrid
        label="Synthetic worklist"
        columns={COLUMNS}
        rows={items}
        getRowId={getRowId}
        fill
      />,
    );
    expect(screen.getByRole("grid").hasAttribute("data-fill")).toBe(true);
  });
});

describe("DataGrid column widths", () => {
  it("keeps a flexible identifier column wider than other flexible columns", () => {
    const identified: readonly DataGridColumn<Item>[] = [
      { id: "id", header: "Accession", cell: (item) => item.id, identifier: true },
      { id: "summary", header: "Summary", cell: (item) => item.summary },
      { id: "count", header: "Results", cell: (item) => item.count, width: 96 },
    ];
    renderWithProviders(
      <DataGrid label="Synthetic worklist" columns={identified} rows={items} getRowId={getRowId} />,
    );
    const header = screen.getAllByRole("row")[0];
    expect(header?.style.gridTemplateColumns).toBe("minmax(12rem, 1fr) minmax(8rem, 1fr) 96px");
    expect(header?.style.minWidth).toContain("96px + 20rem");
  });
});
