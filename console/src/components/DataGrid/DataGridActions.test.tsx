import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import type { ActionItem } from "@/components/RowActions";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRowActions, selectRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderWithProviders } from "@/lib/test/render";
import { DataGrid } from "./DataGrid";
import type { DataGridColumn } from "./gridTypes";
import type { GridRowActions } from "./useRowMenu";

interface Item {
  id: string;
  /** Stands in for the engine's `allowedActions`. */
  allowed: string[];
}

const ITEMS: Item[] = [
  { id: "SYN-A", allowed: ["verify", "retire"] },
  { id: "SYN-B", allowed: [] },
  { id: "SYN-C", allowed: ["retire"] },
];

const COLUMNS: readonly DataGridColumn<Item>[] = [
  { id: "id", header: "Code", identifier: true, cell: (item) => item.id },
  { id: "state", header: "State", cell: (item) => (item.allowed.length ? "Active" : "Retired") },
];

const getRowId = (item: Item) => item.id;

function rowActions(onAction: (id: string, action: string) => void): GridRowActions<Item> {
  return {
    label: (item) => item.id,
    hasItems: (item) => item.allowed.length > 0,
    items: (item): ActionItem[] =>
      item.allowed.map((action) => ({
        id: action,
        label: action === "verify" ? "Verify" : "Retire",
        onAction: () => onAction(item.id, action),
      })),
  };
}

function ActionsGrid({ actions }: { actions: GridRowActions<Item> }) {
  return (
    <DataGrid
      label="Synthetic destinations"
      columns={COLUMNS}
      rows={ITEMS}
      getRowId={getRowId}
      onRowAction={vi.fn()}
      rowActions={actions}
    />
  );
}

const cell = (name: string) => screen.getByRole("gridcell", { name });

describe("DataGrid row actions", () => {
  beforeEach(mockGridLayout);

  it("shows a named menu button only on rows with actions, building items only when opened", async () => {
    const onAction = vi.fn();
    const actions = rowActions(onAction);
    const items = vi.spyOn(actions, "items");
    const { user, container } = renderWithProviders(<ActionsGrid actions={actions} />);
    expect(
      screen.getAllByRole("button").map((button) => button.getAttribute("aria-label")),
    ).toEqual(["Actions for SYN-A", "Actions for SYN-C"]);
    expect(items).not.toHaveBeenCalled();
    await expectNoA11yViolations(container);
    const menu = await openRowActions(user, "SYN-A");
    expect(items).toHaveBeenCalledTimes(1);
    await user.click(within(menu).getByRole("menuitem", { name: "Retire" }));
    expect(onAction).toHaveBeenCalledWith("SYN-A", "retire");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens the same menu on right-click, and keeps the native menu on rows without actions", async () => {
    renderWithProviders(<ActionsGrid actions={rowActions(vi.fn())} />);
    expect(fireEvent.contextMenu(cell("Retired"))).toBe(true);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(fireEvent.contextMenu(cell("SYN-C"))).toBe(false);
    const menu = await screen.findByRole("menu");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Retire"]);
  });

  it("opens the menu with Shift+F10 and returns focus to the cell when it closes", async () => {
    const { user } = renderWithProviders(<ActionsGrid actions={rowActions(vi.fn())} />);
    await user.tab();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(cell("SYN-A"));
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu");
    await waitFor(() => expect(menu.contains(document.activeElement)).toBe(true));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(cell("SYN-A")));
    await user.keyboard("{ContextMenu}");
    expect(await screen.findByRole("menu")).toBeTruthy();
  });

  it("returns focus to the cell after a menu opened with the button closes, and stays one Tab stop", async () => {
    const { user } = renderWithProviders(<ActionsGrid actions={rowActions(vi.fn())} />);
    await openRowActions(user, "SYN-C");
    await user.keyboard("{Escape}");
    const actionsCell = screen.getByRole("button", { name: "Actions for SYN-C" }).parentElement;
    await waitFor(() => expect(document.activeElement).toBe(actionsCell));
    await user.tab();
    expect(document.activeElement).toBe(document.body);
  });
});

const exceptSynB = (id: string) => id !== "SYN-B";

function BulkGrid({ isSelectable }: { isSelectable?: (id: string) => boolean }) {
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [rows, setRows] = useState(ITEMS);
  return (
    <>
      <Button onPress={() => setRows(ITEMS.slice(1))}>Filter</Button>
      <DataGrid
        label="Synthetic billing work"
        columns={COLUMNS}
        rows={rows}
        getRowId={getRowId}
        bulk={{
          selectedIds,
          onChange: setSelectedIds,
          isSelectable,
          actions: <Button>Hand off {selectedIds.size}</Button>,
        }}
      />
    </>
  );
}

describe("DataGrid bulk selection", () => {
  beforeEach(mockGridLayout);

  it("has no check column without bulk actions", () => {
    renderWithProviders(<ActionsGrid actions={rowActions(vi.fn())} />);
    expect(screen.queryByRole("columnheader", { name: "Select all rows" })).toBeNull();
  });

  it("toggles rows with Space and the check cell, and shows the count, the actions and Clear", async () => {
    const { user, container } = renderWithProviders(<BulkGrid />);
    expect(screen.getByRole("status").textContent).toBe("None selected.");
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
    await user.click(screen.getByRole("columnheader", { name: "Code" }));
    await user.keyboard("{ArrowDown} ");
    await selectRow(user, "SYN-C");
    expect(screen.getByRole("status").textContent).toBe("2 selected.");
    expect(screen.getByRole("button", { name: "Hand off 2" })).toBeTruthy();
    await expectNoA11yViolations(container);
    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.getByRole("status").textContent).toBe("None selected.");
    await user.click(screen.getByRole("columnheader", { name: "Select all rows" }));
    expect(screen.getByRole("status").textContent).toBe("3 selected.");
  });

  it("passes by a row that may not be selected, in Space, a click and select all", async () => {
    const { user } = renderWithProviders(<BulkGrid isSelectable={exceptSynB} />);
    await selectRow(user, "SYN-B");
    expect(screen.getByRole("status").textContent).toBe("None selected.");
    await user.click(screen.getByRole("columnheader", { name: "Select all rows" }));
    expect(screen.getByRole("status").textContent).toBe("2 selected.");
    expect(screen.getByRole("button", { name: "Hand off 2" })).toBeTruthy();
    expect(
      screen.getByRole("gridcell", { name: "SYN-B" }).closest("[role=row]")?.ariaSelected,
    ).toBeNull();
  });

  it("unselects rows that leave the list, and says so", async () => {
    const { user } = renderWithProviders(<BulkGrid />);
    await selectRow(user, "SYN-A");
    await selectRow(user, "SYN-B");
    await user.click(screen.getByRole("button", { name: "Filter" }));
    expect(screen.getByRole("status").textContent).toBe(
      "1 selected. Unselected 1 row no longer listed.",
    );
    await selectRow(user, "SYN-C");
    expect(screen.getByRole("status").textContent).toBe("2 selected.");
  });
});
