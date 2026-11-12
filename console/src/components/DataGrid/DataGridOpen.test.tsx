import { screen, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRow, openRowWithEnter } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderWithProviders } from "@/lib/test/render";
import { DataGrid } from "./DataGrid";
import type { DataGridColumn } from "./gridTypes";

interface Item {
  id: string;
  name: string;
}

const ITEMS: Item[] = ["SYN-A", "SYN-B", "SYN-C", "SYN-D"].map((id, index) => ({
  id,
  name: `Synthetic analyte ${index + 1}`,
}));

const getRowId = (item: Item) => item.id;

function columns(onCell: (id: string) => void = () => undefined): DataGridColumn<Item>[] {
  return [
    {
      id: "id",
      header: "Code",
      identifier: true,
      cell: (item) => {
        onCell(item.id);
        return item.id;
      },
      sortValue: (item) => item.id,
    },
    {
      id: "name",
      header: "Name",
      cell: (item) => (
        <>
          {item.name} <button type="button">Copy</button>
        </>
      ),
    },
  ];
}

const COLUMNS = columns();

/** A list with a stand-in side panel, as a directory screen composes them. */
function Directory({ onOpen = vi.fn() }: { onOpen?: (id: string) => void }) {
  const [openId, setOpenId] = useState<string | null>(null);
  return (
    <>
      <DataGrid
        label="Synthetic analytes"
        columns={COLUMNS}
        rows={ITEMS}
        getRowId={getRowId}
        openRowId={openId}
        onRowAction={(item) => {
          onOpen(item.id);
          setOpenId(item.id);
        }}
      />
      {openId && (
        <section aria-label="Open record">
          <Button onPress={() => setOpenId(null)}>Close</Button>
        </section>
      )}
    </>
  );
}

const cell = (name: string) => screen.getByRole("gridcell", { name });
const rowOf = (name: string) => cell(name).closest<HTMLElement>("[role=row]");

describe("DataGrid opening records", () => {
  beforeEach(mockGridLayout);

  it("opens a row by a click anywhere on it, and by Enter", async () => {
    const onOpen = vi.fn();
    const { user } = renderWithProviders(<Directory onOpen={onOpen} />);
    await openRow(user, "Synthetic analyte 2 Copy");
    expect(onOpen).toHaveBeenLastCalledWith("SYN-B");
    await openRowWithEnter(user, "SYN-C");
    expect(onOpen).toHaveBeenLastCalledWith("SYN-C");
  });

  it("does not open a row from a control inside it or after selecting text", async () => {
    const onOpen = vi.fn();
    const { user } = renderWithProviders(<Directory onOpen={onOpen} />);
    const row = rowOf("SYN-A");
    if (!row) throw new Error("no row");
    await user.click(within(row).getByRole("button", { name: "Copy" }));
    expect(onOpen).not.toHaveBeenCalled();
    // The grid is still one Tab stop: focus on the control moved to its cell.
    expect(document.activeElement?.getAttribute("role")).toBe("gridcell");
    // Drag across "SYN-A" to select it for copying.
    await user.pointer([
      { keys: "[MouseLeft>]", target: cell("SYN-A"), offset: 0 },
      { offset: 5 },
      { keys: "[/MouseLeft]" },
    ]);
    expect(window.getSelection()?.toString()).toBe("SYN-A");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("marks the open row for assistive technology, without violations", async () => {
    const { user, container } = renderWithProviders(<Directory />);
    await openRow(user, "SYN-B");
    expect(rowOf("SYN-B")?.getAttribute("aria-current")).toBe("true");
    expect(rowOf("SYN-A")?.hasAttribute("aria-current")).toBe(false);
    await expectNoA11yViolations(container);
  });

  it("returns focus to the open row when the panel holding focus closes", async () => {
    const { user } = renderWithProviders(<Directory />);
    await openRow(user, "SYN-C");
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("region", { name: "Open record" })).toBeNull();
    expect(document.activeElement).toBe(cell("SYN-C"));
    expect(rowOf("SYN-C")?.hasAttribute("aria-current")).toBe(false);
  });

  it("re-renders only the old and the new open rows when another row opens", async () => {
    const rendered = vi.fn();
    const counted = columns(rendered);
    // Row actions and bulk selection given inline, as screens will, keep rows memoized too.
    function Counted() {
      const [openId, setOpenId] = useState<string | null>(null);
      const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
      return (
        <DataGrid
          label="Synthetic analytes"
          columns={counted}
          rows={ITEMS}
          getRowId={getRowId}
          openRowId={openId}
          onRowAction={(item) => setOpenId(item.id)}
          rowActions={{ label: (item) => item.id, hasItems: () => true, items: () => [] }}
          bulk={{ selectedIds, onChange: setSelectedIds, actions: null }}
        />
      );
    }
    const { user } = renderWithProviders(<Counted />);
    await openRow(user, "SYN-A");
    rendered.mockClear();
    await openRow(user, "SYN-C");
    // SYN-A loses the open mark and the tab stop; SYN-C gains both.
    expect(new Set(rendered.mock.calls.map(([id]) => id))).toEqual(new Set(["SYN-A", "SYN-C"]));
  });

  it("styles the identifier column as a link without making it one", () => {
    renderWithProviders(<Directory />);
    expect(cell("SYN-A").hasAttribute("data-identifier")).toBe(true);
    expect(screen.queryByRole("link")).toBeNull();
  });
});
