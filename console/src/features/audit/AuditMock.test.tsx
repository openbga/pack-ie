import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { AuditPage } from "./AuditPage";

const ROUTES = [{ path: "/audit", element: <AuditPage /> }];

const loaded = () => {
  const text = screen
    .getAllByRole("status")
    .find((element) => /events? loaded/.test(element.textContent ?? ""));
  return Number(
    /^([\d,]+) event/.exec(text?.textContent ?? "")?.[1]?.replace(",", "") ?? Number.NaN,
  );
};

/** The Audit screen against the mock engine's seeded log. */
describe("Audit against the mock engine", () => {
  let api: MockEngineApi;
  beforeEach(() => {
    mockGridLayout();
    api = new MockEngineApi({ latency: "none", messageCount: 500, seed: 7 });
  });
  afterEach(() => api.dispose());

  it("shows the newest 100 events, then a page more for each Load older", async () => {
    const { user } = renderRoutes(ROUTES, "/audit", { api });
    await screen.findByRole("grid", { name: "Audit events" });
    expect(loaded()).toBe(100);
    await user.click(screen.getByRole("button", { name: "Load older events" }));
    await waitFor(() => expect(loaded()).toBe(200));
  });

  it("filters by action on the engine", async () => {
    renderRoutes(ROUTES, "/audit?action=config_apply", { api });
    await screen.findByRole("grid", { name: "Audit events" });
    const rows = gridRows("Audit events");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(within(row).getByText("Configuration applied")).toBeTruthy();
    }
  });

  it("lists what the console just did, newest first", async () => {
    await api.pauseChannel("ch-msg-sta");
    renderRoutes(ROUTES, "/audit", { api });
    await screen.findByRole("grid", { name: "Audit events" });
    const [newest, second] = gridRows("Audit events");
    expect(within(newest as HTMLElement).getByText("Channel paused")).toBeTruthy();
    expect(within(newest as HTMLElement).getByText("Succeeded")).toBeTruthy();
    expect(within(second as HTMLElement).getByText("Requested")).toBeTruthy();
  });

  it("verifies the chain and records it", async () => {
    const { user } = renderRoutes(ROUTES, "/audit", { api });
    await screen.findByRole("grid", { name: "Audit events" });
    await user.click(screen.getByRole("button", { name: "Verify chain" }));
    expect(await screen.findByText("Chain verified: OK")).toBeTruthy();
    await waitFor(() => {
      const first = within(screen.getByRole("grid")).getAllByRole("row")[1] as HTMLElement;
      expect(within(first).getByText("Audit chain verified")).toBeTruthy();
    });
  });
});
