import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { PagingBar } from "./PagingBar";

function bar(overrides: Partial<Parameters<typeof PagingBar>[0]> = {}) {
  const onOffsetChange = vi.fn();
  const onPageSizeChange = vi.fn();
  const element = (
    <PagingBar
      offset={0}
      rowCount={100}
      total={412_455}
      totalExact
      pageSize={100}
      pageSizes={[50, 100, 200]}
      noun="Rows"
      onOffsetChange={onOffsetChange}
      onPageSizeChange={onPageSizeChange}
      {...overrides}
    />
  );
  return { element, onOffsetChange, onPageSizeChange };
}

const button = (name: string) => screen.getByRole("button", { name });
const disabled = (name: string) => button(name).hasAttribute("disabled");

describe("PagingBar", () => {
  it("states the range against the exact count and starts on the first page", async () => {
    const { element } = bar();
    const { container } = renderWithProviders(element);
    expect(screen.getByRole("status").textContent).toBe("Rows 1–100 of 412,455");
    expect(disabled("First")).toBe(true);
    expect(disabled("Previous")).toBe(true);
    expect(disabled("Next")).toBe(false);
    await expectNoA11yViolations(container);
  });

  it("asks for the next, previous and first page by offset", async () => {
    const { element, onOffsetChange } = bar({ offset: 200 });
    const { user } = renderWithProviders(element);
    expect(screen.getByRole("status").textContent).toBe("Rows 201–300 of 412,455");
    await user.click(button("Next"));
    expect(onOffsetChange).toHaveBeenLastCalledWith(300);
    await user.click(button("Previous"));
    expect(onOffsetChange).toHaveBeenLastCalledWith(100);
    await user.click(button("First"));
    expect(onOffsetChange).toHaveBeenLastCalledWith(0);
  });

  it("never goes before the first row from a partial page offset", async () => {
    const { element, onOffsetChange } = bar({ offset: 40, rowCount: 100 });
    const { user } = renderWithProviders(element);
    await user.click(button("Previous"));
    expect(onOffsetChange).toHaveBeenLastCalledWith(0);
  });

  it("ends on the last page of an exact count", () => {
    const { element } = bar({ offset: 412_400, rowCount: 55 });
    renderWithProviders(element);
    expect(screen.getByRole("status").textContent).toBe("Rows 412,401–412,455 of 412,455");
    expect(disabled("Next")).toBe(true);
    expect(disabled("Previous")).toBe(false);
  });

  it("stops at a full page that ends exactly on the count", () => {
    const { element } = bar({ offset: 100, rowCount: 100, total: 200 });
    renderWithProviders(element);
    expect(disabled("Next")).toBe(true);
  });

  it("never presents a capped count as exact, and keeps Next while full pages arrive", () => {
    const { element } = bar({ total: 1_000, totalExact: false });
    renderWithProviders(element);
    expect(screen.getByRole("status").textContent).toBe("Rows 1–100 of 1,000 or more");
    expect(disabled("Next")).toBe(false);
  });

  it("ends a capped count on a short page", () => {
    const { element } = bar({ offset: 1_000, rowCount: 12, total: 1_012, totalExact: false });
    renderWithProviders(element);
    expect(disabled("Next")).toBe(true);
  });

  it("says so when a page is empty, and shows that a newer answer is loading", () => {
    const { element } = bar({ offset: 500, rowCount: 0, total: 10, isRefreshing: true });
    renderWithProviders(element);
    expect(screen.getByRole("status").textContent).toBe("No rows on this page · Updating…");
    expect(disabled("Next")).toBe(true);
  });

  it("offers the page sizes and reports a new one", async () => {
    const { element, onPageSizeChange } = bar();
    const { user } = renderWithProviders(element);
    await user.click(screen.getByRole("button", { name: /Rows per page/ }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "50",
      "100",
      "200",
    ]);
    await user.click(screen.getByRole("option", { name: "200" }));
    expect(onPageSizeChange).toHaveBeenCalledWith(200);
  });

  it("is operable with the keyboard", async () => {
    const { element, onOffsetChange } = bar();
    const { user } = renderWithProviders(element);
    const next = button("Next");
    await user.tab();
    expect(document.activeElement).toBe(next);
    await user.keyboard("{Enter}");
    expect(onOffsetChange).toHaveBeenCalledWith(100);
  });
});
