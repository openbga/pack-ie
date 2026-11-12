import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { act, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IdleProvider } from "@/lib/idle";
import { renderRoutes } from "@/lib/test/router";
import { useLeaveOnIdle } from "./useLeaveOnIdle";

let cache: QueryClient;

function Record() {
  cache = useQueryClient();
  useLeaveOnIdle("/list?state=failed", "getMessage");
  return <h1>Record</h1>;
}

describe("useLeaveOnIdle", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => vi.useRealTimers());

  function setup() {
    const rendered = renderRoutes(
      [
        {
          path: "/record",
          element: (
            <IdleProvider>
              <Record />
            </IdleProvider>
          ),
        },
      ],
      "/record",
    );
    // Entries nobody observes would be collected by the fake clock's fifteen minutes.
    cache.setDefaultOptions({ queries: { gcTime: Number.POSITIVE_INFINITY } });
    cache.setQueryData(["getMessage", "m1"], { id: "m1" });
    cache.setQueryData(["getMessageFields", "m1"], { fields: [] });
    cache.setQueryData(["listMessages"], { rows: [] });
    return rendered;
  }

  it("leaves for the list when the console goes idle, replacing the record in the history", async () => {
    const { router } = setup();
    act(() => void vi.advanceTimersByTime(14 * 60_000));
    expect(router.state.location.pathname).toBe("/record");
    act(() => void vi.advanceTimersByTime(61_000));
    await screen.findByText("Navigated to /list?state=failed");
    expect(router.state.historyAction).toBe("REPLACE");
    expect(router.state.location.search).toBe("?state=failed");
  });

  it("drops the cached record once the screen is gone, and only that method", async () => {
    setup();
    expect(cache.getQueryData(["getMessage", "m1"])).toBeDefined();
    act(() => void vi.advanceTimersByTime(15 * 60_000 + 1000));
    await screen.findByText("Navigated to /list?state=failed");
    expect(cache.getQueryData(["getMessage", "m1"])).toBeUndefined();
    expect(cache.getQueryData(["listMessages"])).toBeDefined();
  });

  it("keeps the cache when the screen is left any other way", async () => {
    const { router } = setup();
    await act(async () => router.navigate("/elsewhere"));
    expect(cache.getQueryData(["getMessage", "m1"])).toBeDefined();
  });
});
