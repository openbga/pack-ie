import { screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { DashboardPage } from "./DashboardPage";

/** The dashboard against the mock engine's snapshot, with its channels in every state. */
describe("Dashboard against the mock engine", () => {
  let api: MockEngineApi;
  beforeEach(() => {
    mockGridLayout();
    api = new MockEngineApi({ latency: "none", messageCount: 50, logRingSize: 200 });
  });
  afterEach(() => api.dispose());

  it("shows every channel in one table, nothing invented, failing ones before running ones", async () => {
    renderRoutes([{ path: "/dashboard", element: <DashboardPage /> }], "/dashboard", { api });
    await screen.findByRole("grid", { name: "Channels" });
    const snapshot = await api.getSnapshot();
    const rows = gridRows("Channels");
    expect(rows.length).toBe(snapshot.channels.length);
    for (const row of rows) {
      expect(row.textContent).not.toMatch(/undefined|NaN|\[object/);
    }
    const stateOf = (row: HTMLElement) =>
      within(row).getAllByRole("gridcell")[1]?.textContent ?? "";
    // Retrying and degraded channels come before any channel that is merely running.
    const running = rows.filter((row) => stateOf(row).startsWith("Running"));
    const trouble = rows.filter((row) => /^(Retrying|Degraded)/.test(stateOf(row)));
    for (const row of trouble) {
      for (const calm of running) {
        expect(rows.indexOf(row)).toBeLessThan(rows.indexOf(calm));
      }
    }
  });
});
