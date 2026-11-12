import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { LogsPage } from "./LogsPage";

const ROUTES = [{ path: "/logs", element: <LogsPage /> }];

const loaded = () => {
  const text = screen
    .getAllByRole("status")
    .find((element) => /loaded/.test(element.textContent ?? ""));
  return Number(
    /^([\d,]+) loaded/.exec(text?.textContent ?? "")?.[1]?.replace(",", "") ?? Number.NaN,
  );
};

/** The Logs screen against the mock engine's log ring and live stream. */
describe("Logs against the mock engine", () => {
  let api: MockEngineApi;
  beforeEach(() => {
    mockGridLayout();
    api = new MockEngineApi({ latency: "none", messageCount: 200, logRingSize: 2000 });
  });
  afterEach(() => api.dispose());

  it("shows the newest 200 events, then more as the engine logs them", async () => {
    renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    expect(loaded()).toBe(200);
    await waitFor(() => expect(loaded()).toBeGreaterThan(200), { timeout: 4000 });
  });

  it("loads older events a page at a time", async () => {
    const { user } = renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    await user.click(screen.getByRole("button", { name: "Load older events" }));
    await waitFor(() => expect(loaded()).toBeGreaterThanOrEqual(400));
  });

  it("filters to errors from the URL, naming each event's channel and level", async () => {
    renderRoutes(ROUTES, "/logs?level=error", { api });
    await screen.findByRole("grid", { name: "Log events" });
    const rows = gridRows("Log events");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) expect(within(row).getByText("Error")).toBeTruthy();
  });
});
