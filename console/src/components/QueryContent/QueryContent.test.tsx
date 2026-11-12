import type { UseQueryResult } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { EngineError } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { QueryContent } from "./QueryContent";

type Query = UseQueryResult<string[], EngineError>;

// Only the fields QueryContent reads; the rest of a query result is irrelevant here.
function query(fields: Partial<Record<keyof Query, unknown>>): Query {
  const base = {
    isPending: false,
    isLoadingError: false,
    isRefetchError: false,
    data: undefined,
    error: null,
    refetch: vi.fn(),
  };
  return Object.assign(Object.create(null), base, fields);
}

const EMPTY = {
  when: (rows: string[]) => rows.length === 0,
  title: "No rows",
  description: "None.",
};

const FAILURE = engineError("internal", "Synthetic failure.");

describe("QueryContent", () => {
  it("shows loading, then the content", async () => {
    const { rerender, container } = render(
      <QueryContent query={query({ isPending: true })} what="rows">
        {(rows) => <p>{rows.join(", ")}</p>}
      </QueryContent>,
    );
    expect(screen.getByText("Loading rows…")).toBeTruthy();
    rerender(
      <QueryContent query={query({ data: ["a", "b"] })} what="rows" empty={EMPTY}>
        {(rows) => <p>{rows.join(", ")}</p>}
      </QueryContent>,
    );
    expect(screen.getByText("a, b")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("shows the empty state", () => {
    render(
      <QueryContent query={query({ data: [] })} what="rows" empty={EMPTY}>
        {() => <p>content</p>}
      </QueryContent>,
    );
    expect(screen.getByText("No rows")).toBeTruthy();
  });

  it("offers the empty state's next step", async () => {
    const showAll = vi.fn();
    const { container } = render(
      <QueryContent
        query={query({ data: [] })}
        what="rows"
        empty={{
          ...EMPTY,
          title: "Nothing matches",
          action: (
            <button type="button" onClick={showAll}>
              Show all
            </button>
          ),
        }}
      >
        {() => <p>content</p>}
      </QueryContent>,
    );
    expect(screen.getByText("Nothing matches")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(showAll).toHaveBeenCalled();
    await expectNoA11yViolations(container);
  });

  it("shows the engine's error with a retry", async () => {
    const refetch = vi.fn();
    render(
      <QueryContent query={query({ isLoadingError: true, error: FAILURE, refetch })} what="rows">
        {() => <p>content</p>}
      </QueryContent>,
    );
    expect(screen.getByText("The rows could not be loaded")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(refetch).toHaveBeenCalled();
  });

  it("keeps the rows shown when a refresh fails, and says so", async () => {
    const refetch = vi.fn();
    const { container } = render(
      <QueryContent
        query={query({ isRefetchError: true, data: ["a", "b"], error: FAILURE, refetch })}
        what="rows"
      >
        {(rows) => <p>{rows.join(", ")}</p>}
      </QueryContent>,
    );
    expect(screen.getByText("a, b")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain(
      "Could not refresh. The rows shown may be out of date.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(refetch).toHaveBeenCalled();
    await expectNoA11yViolations(container);
  });

  it("keeps the page heading while loading and after a failure", async () => {
    const { rerender, container } = render(
      <QueryContent query={query({ isPending: true })} what="report" pageTitle="Report">
        {() => <h1>Report SYN-000001</h1>}
      </QueryContent>,
    );
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Report");
    rerender(
      <QueryContent
        query={query({ isLoadingError: true, error: FAILURE })}
        what="report"
        pageTitle="Report"
      >
        {() => <h1>Report SYN-000001</h1>}
      </QueryContent>,
    );
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Report");
    await expectNoA11yViolations(container);
    rerender(
      <QueryContent query={query({ data: [] })} what="report" pageTitle="Report">
        {() => <h1>Report SYN-000001</h1>}
      </QueryContent>,
    );
    expect(screen.getAllByRole("heading", { level: 1 }).map((h) => h.textContent)).toEqual([
      "Report SYN-000001",
    ]);
  });

  it("shows placeholder rows while a grid loads", () => {
    render(
      <QueryContent query={query({ isPending: true })} what="rows" loadingRows={6}>
        {() => <p>content</p>}
      </QueryContent>,
    );
    expect(screen.getByTestId("skeleton")).toBeTruthy();
  });
});
