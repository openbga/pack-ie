import { screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { FindCount, FindField, findRows } from "@/components/FindField";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderWithProviders } from "@/lib/test/render";
import { DataGrid } from "./DataGrid";
import type { DataGridColumn } from "./gridTypes";

interface Item {
  id: string;
  name: string;
}

const ITEMS: Item[] = [
  { id: "SYN-1", name: "Synthetic glucose" },
  { id: "SYN-2", name: "Synthetic sodium" },
  { id: "SYN-3", name: "Synthetic Glucose tolerance" },
];

const COLUMNS: readonly DataGridColumn<Item>[] = [
  {
    id: "id",
    header: "Code",
    identifier: true,
    cell: (item) => item.id,
    sortValue: (item) => item.id,
  },
  { id: "name", header: "Name", cell: (item) => item.name, sortValue: (item) => item.name },
];

const getRowId = (item: Item) => item.id;
const fields = (item: Item) => [item.id, item.name];

/** A complete collection the screen filters itself: Find, the narrowed rows, one count. */
function Directory({ rows = ITEMS }: { rows?: Item[] }) {
  const [query, setQuery] = useState("");
  const shown = findRows(rows, query, fields);
  return (
    <>
      <FindField
        label="Find tests"
        value={query}
        onChange={setQuery}
        count={{ shown: shown.length, total: rows.length, noun: "tests", one: "test" }}
      />
      <DataGrid label="Synthetic tests" columns={COLUMNS} rows={shown} getRowId={getRowId} />
    </>
  );
}

const codes = () => gridRows().map((row) => row.textContent?.slice(0, 5));

describe("DataGrid with Find", () => {
  beforeEach(mockGridLayout);

  it("narrows the rows to those where any field contains the text, ignoring case", async () => {
    const { user } = renderWithProviders(<Directory />);
    expect(codes()).toEqual(["SYN-1", "SYN-2", "SYN-3"]);
    expect(screen.getByRole("status").textContent).toBe("3 tests");
    await user.type(screen.getByRole("searchbox", { name: "Find tests" }), "GLUCOSE");
    await waitFor(() => expect(codes()).toEqual(["SYN-1", "SYN-3"]));
    expect(screen.getByRole("status").textContent).toBe("2 of 3 tests");
  });

  it("matches the code as well as the name, and keeps a sort of the narrowed rows", async () => {
    const { user } = renderWithProviders(<Directory />);
    await user.type(screen.getByRole("searchbox", { name: "Find tests" }), "syn-2");
    await waitFor(() => expect(codes()).toEqual(["SYN-2"]));
    await user.clear(screen.getByRole("searchbox", { name: "Find tests" }));
    await user.type(screen.getByRole("searchbox", { name: "Find tests" }), "glucose");
    await waitFor(() => expect(codes()).toEqual(["SYN-1", "SYN-3"]));
    await user.click(screen.getByRole("columnheader", { name: "Name" }));
    expect(codes()).toEqual(["SYN-1", "SYN-3"]);
    await user.click(screen.getByRole("columnheader", { name: "Name" }));
    expect(codes()).toEqual(["SYN-3", "SYN-1"]);
  });

  it("shows no rows when nothing matches, and Escape clears the text", async () => {
    const { user } = renderWithProviders(<Directory />);
    await user.type(screen.getByRole("searchbox", { name: "Find tests" }), "zzz");
    await waitFor(() => expect(codes()).toEqual([]));
    expect(screen.getByRole("status").textContent).toBe("0 of 3 tests");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(codes()).toEqual(["SYN-1", "SYN-2", "SYN-3"]));
  });

  it("words the count for one row in the singular", () => {
    renderWithProviders(<FindCount shown={1} total={1} noun="tests" one="test" />);
    expect(screen.getByText("1 test")).toBeTruthy();
  });
});
