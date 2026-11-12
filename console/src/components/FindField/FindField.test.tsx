import { screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { FindField } from "./FindField";
import { findRows } from "./findRows";
import { NoResults } from "./NoResults";

const field = () => screen.getByRole("searchbox", { name: "Find analytes" });

function Harness({ onChange }: { onChange: (value: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <>
      <FindField
        label="Find analytes"
        value={value}
        onChange={(next) => {
          setValue(next);
          onChange(next);
        }}
        count={{ shown: value === "" ? 340 : 12, total: 340, noun: "analytes", one: "analyte" }}
      />
      <button type="button" onClick={() => setValue("")}>
        Reset from outside
      </button>
    </>
  );
}

describe("FindField", () => {
  it("shows a name for the criterion instead of Find, and is then not a search landmark", () => {
    const { container } = renderWithProviders(
      <FindField label="Identifier" visibleLabel="Identifier" value="" onChange={vi.fn()} />,
    );
    expect(screen.getByRole("searchbox", { name: "Identifier" })).toBeTruthy();
    expect(screen.getByText("Identifier", { selector: "label [aria-hidden]" })).toBeTruthy();
    expect(screen.queryByText("Find")).toBeNull();
    expect(container.querySelector("search")).toBeNull();
  });

  it("says Find by default and is a search landmark", () => {
    const { container } = renderWithProviders(
      <FindField label="Find analytes" value="" onChange={vi.fn()} />,
    );
    expect(screen.getByText("Find", { selector: "label [aria-hidden]" })).toBeTruthy();
    expect(container.querySelector("search")).not.toBeNull();
  });

  it("finds once typing pauses, and counts the rows in a live region", async () => {
    const onChange = vi.fn();
    const { user, container } = renderWithProviders(<Harness onChange={onChange} />);
    expect(screen.getByRole("status").textContent).toBe("340 analytes");
    await user.type(field(), "oxy");
    expect(onChange).not.toHaveBeenCalled();
    await waitFor(() => expect(onChange).toHaveBeenCalledWith("oxy"));
    expect(onChange).toHaveBeenCalledOnce();
    // The callback can finish before React commits the updated live-region count.
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("12 of 340 analytes"));
    await expectNoA11yViolations(container);
  });

  it("clears at once with Escape", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await user.type(field(), "oxy{Enter}");
    expect(onChange).toHaveBeenLastCalledWith("oxy");
    await user.keyboard("{Escape}");
    expect(onChange).toHaveBeenLastCalledWith("");
    expect(field()).toHaveProperty("value", "");
  });

  it("shows text set from outside, such as by Clear search", async () => {
    const { user } = renderWithProviders(<Harness onChange={() => undefined} />);
    await user.type(field(), "oxy{Enter}");
    await user.click(screen.getByRole("button", { name: "Reset from outside" }));
    expect(field()).toHaveProperty("value", "");
  });
});

describe("FindField while typing", () => {
  it("keeps the space typed after a word when the pause reports the trimmed text", async () => {
    const onChange = vi.fn();
    function Trimming() {
      const [value, setValue] = useState("");
      return (
        <FindField
          label="Find analytes"
          value={value}
          onChange={(next) => {
            setValue(next.trim());
            onChange(next.trim());
          }}
        />
      );
    }
    const { user } = renderWithProviders(<Trimming />);
    await user.type(field(), "blood ");
    await waitFor(() => expect(onChange).toHaveBeenCalledWith("blood"));
    expect(field()).toHaveProperty("value", "blood ");
    await user.type(field(), "gas");
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith("blood gas"));
    expect(field()).toHaveProperty("value", "blood gas");
  });

  it("drops text typed but not yet reported when the value is reset from outside", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await user.type(field(), "oxy{Enter}");
    await user.type(field(), "g");
    await user.click(screen.getByRole("button", { name: "Reset from outside" }));
    expect(field()).toHaveProperty("value", "");
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(onChange).not.toHaveBeenCalledWith("oxyg");
  });
});

describe("findRows", () => {
  const rows = [
    { code: "SYN-OXY", name: "Synthetic analyte one", loinc: null },
    { code: "SYN-MOR", name: "Synthetic analyte two", loinc: "0000-1" },
  ];
  const fields = (row: (typeof rows)[number]) => [row.code, row.name, row.loinc];

  it("matches any field, ignoring case and surrounding spaces", () => {
    expect(findRows(rows, " syn-oxy ", fields)).toEqual([rows[0]]);
    expect(findRows(rows, "TWO", fields)).toEqual([rows[1]]);
    expect(findRows(rows, "0000", fields)).toEqual([rows[1]]);
    expect(findRows(rows, "absent", fields)).toEqual([]);
  });

  it("keeps every row for blank text", () => {
    expect(findRows(rows, "  ", fields)).toBe(rows);
  });
});

describe("NoResults", () => {
  it("names the search and clears it", async () => {
    const onClear = vi.fn();
    const { user, container } = renderWithProviders(
      <NoResults noun="analytes" query="oxy" onClear={onClear} />,
    );
    expect(screen.getByText("No analytes match 'oxy'")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onClear).toHaveBeenCalledOnce();
    await expectNoA11yViolations(container);
  });

  it("offers to clear the filters when there is no find text", () => {
    renderWithProviders(<NoResults noun="analytes" query="" onClear={() => undefined} />);
    expect(screen.getByText("No analytes match the filters")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Clear filters" })).toBeTruthy();
  });
});
