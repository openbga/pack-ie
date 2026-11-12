import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { RefreshControl } from "./RefreshControl";

describe("RefreshControl", () => {
  it("says when the data was read and refreshes on request", async () => {
    const refetch = vi.fn();
    const dataUpdatedAt = Date.now();
    const { user, container } = renderWithProviders(
      <RefreshControl query={{ dataUpdatedAt, isFetching: false, refetch }} />,
    );
    // Read today, so only the time shows, in UTC without a laboratory zone.
    expect(screen.getByRole("status").textContent).toMatch(/^Updated \d{2}:\d{2} UTC$/);
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(refetch).toHaveBeenCalledOnce();
    await expectNoA11yViolations(container);
  });

  it("shows the date of an older read, and nothing before the first", () => {
    const { rerender } = renderWithProviders(
      <RefreshControl query={{ dataUpdatedAt: 0, isFetching: true, refetch: vi.fn() }} />,
    );
    expect(screen.getByRole("status").textContent).toBe("");
    rerender(
      <RefreshControl
        query={{
          dataUpdatedAt: Date.parse("2026-01-02T03:04:00Z"),
          isFetching: false,
          refetch: vi.fn(),
        }}
      />,
    );
    expect(screen.getByRole("status").textContent).toBe("Updated 2026-01-02 03:04 UTC");
  });

  it("blocks repeated presses while a refresh runs", async () => {
    const refetch = vi.fn();
    const { user } = renderWithProviders(
      <RefreshControl query={{ dataUpdatedAt: Date.now(), isFetching: true, refetch }} />,
    );
    await user.click(screen.getByRole("button", { name: /Refresh/ }));
    expect(refetch).not.toHaveBeenCalled();
  });
});
