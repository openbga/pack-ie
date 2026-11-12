import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRow } from "@/lib/test/grid";
import { chooseWithKeyboard } from "@/lib/test/keyboard";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { ago, alert, engineError, snapshot, stubApi } from "../shared-operate/testFixtures";
import { AlertsPage } from "./AlertsPage";

const routes = [{ path: "/alerts", element: <AlertsPage /> }];

const open = [
  alert({
    id: "a-warn",
    severity: "warning",
    title: "Queue filling",
    kind: "delivery_backlog",
    channelId: "ch-alpha",
    endpointId: null,
    lastSeenAt: ago(1),
  }),
  alert({ id: "a-crit", severity: "critical", title: "Endpoint down" }),
];
const cleared = [
  alert({
    id: "a-old",
    severity: "info",
    title: "Disk space low",
    state: "cleared",
    clearedAt: ago(100),
    channelId: null,
    endpointId: null,
  }),
];

function api(overrides: Record<string, unknown> = {}) {
  return stubApi({
    listAlerts: vi.fn(async (state: string) =>
      state === "open" ? open : state === "cleared" ? cleared : [...open, ...cleared],
    ),
    getSnapshot: async () => snapshot(),
    ...overrides,
  });
}

const titles = () =>
  within(screen.getByRole("grid", { name: "Alerts" }))
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("gridcell")[1]?.firstElementChild?.textContent);

beforeEach(() => mockGridLayout());

describe("AlertsPage", () => {
  it("lists open alerts, most severe first, with what they concern and the next step", async () => {
    const { container } = renderRoutes(routes, "/alerts", { api: api() });
    expect(await screen.findByRole("grid", { name: "Alerts" })).toBeTruthy();
    expect(titles()).toEqual(["Endpoint down", "Queue filling"]);
    const grid = screen.getByRole("grid", { name: "Alerts" });
    expect(within(grid).getByText("Critical")).toBeTruthy();
    const concerns = within(grid)
      .getByRole("link", { name: "Analyzer COM3" })
      .closest("[role=gridcell]");
    expect(concerns?.textContent).toBe("Channel Alpha bridge · Endpoint Analyzer COM3");
    expect(
      within(grid).getAllByRole("link", { name: "Alpha bridge" })[0]?.getAttribute("href"),
    ).toBe("/channels/ch-alpha");
    expect(within(grid).getByRole("link", { name: "Analyzer COM3" }).getAttribute("href")).toBe(
      "/endpoints/ep-com3",
    );
    expect(within(grid).getAllByText(/Check the serial cable/)).toHaveLength(2);
    expect(screen.getByText("2 alerts")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("puts the next step right after the summary, with the detail under the title", async () => {
    renderRoutes(routes, "/alerts", { api: api() });
    const grid = await screen.findByRole("grid", { name: "Alerts" });
    expect(
      within(grid)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual([
      "Severity",
      "Alert",
      "Next step",
      "State",
      "Concerns",
      "First seen",
      "Last seen",
      "Seen",
    ]);
    const row = within(grid).getAllByRole("row")[1] as HTMLElement;
    const cells = within(row).getAllByRole("gridcell");
    expect(cells[1]?.textContent).toBe(
      "Endpoint down Analyzer COM3 has been disconnected for 10 minutes.",
    );
    expect(cells[2]?.textContent).toBe("Check the serial cable and the analyzer's power.");
    expect(within(row).getByTitle("Channel Alpha bridge · Endpoint Analyzer COM3")).toBeTruthy();
  });

  it("shows first and last seen as compact timestamps with the whole time in the title", async () => {
    renderRoutes(routes, "/alerts", { api: api() });
    const grid = await screen.findByRole("grid", { name: "Alerts" });
    const row = within(grid).getAllByRole("row")[1] as HTMLElement;
    const times = row.querySelectorAll("time");
    expect(times).toHaveLength(2);
    for (const time of times) {
      expect(time.textContent).toMatch(/^\d\d-\d\d \d\d:\d\d/);
      expect(time.getAttribute("title")).toMatch(/\d{4}/);
    }
  });

  it("orders open alerts critical, then warning, then first seen, and lists cleared ones after them", async () => {
    const mixed = [
      alert({ id: "w-late", severity: "warning", title: "Warn late", firstSeenAt: ago(10) }),
      alert({ id: "w-early", severity: "warning", title: "Warn early", firstSeenAt: ago(500) }),
      alert({ id: "c-late", severity: "critical", title: "Crit late", firstSeenAt: ago(20) }),
      alert({ id: "c-early", severity: "critical", title: "Crit early", firstSeenAt: ago(900) }),
      ...cleared,
    ];
    renderRoutes(routes, "/alerts?state=all", { api: api({ listAlerts: async () => mixed }) });
    await screen.findByRole("grid", { name: "Alerts" });
    expect(titles()).toEqual([
      "Crit early",
      "Crit late",
      "Warn early",
      "Warn late",
      "Disk space low",
    ]);
  });

  it("shows how many times an alert was raised, and says when the engine does not count", async () => {
    renderRoutes(routes, "/alerts", {
      api: api({
        listAlerts: async () => [
          alert({ id: "a", title: "Counted", occurrences: 4 }),
          alert({ id: "b", severity: "warning", title: "Uncounted", occurrences: null }),
        ],
      }),
    });
    const grid = await screen.findByRole("grid", { name: "Alerts" });
    expect(within(grid).getByText("4×")).toBeTruthy();
    expect(within(grid).getByText("Not counted")).toBeTruthy();
  });

  it("switches between open, cleared and all alerts, kept in the address", async () => {
    const listAlerts = vi.fn(async (state: string) =>
      state === "open" ? open : [...open, ...cleared],
    );
    const { user, router } = renderRoutes(routes, "/alerts", { api: api({ listAlerts }) });
    await screen.findByRole("grid", { name: "Alerts" });

    await chooseWithKeyboard(user, /State/, "All");
    await waitFor(() => expect(titles()).toHaveLength(3));
    expect(listAlerts).toHaveBeenCalledWith("all");
    expect(router.state.location.search).toBe("?state=all");
    expect(within(screen.getByRole("grid", { name: "Alerts" })).getByText("Cleared")).toBeTruthy();
  });

  it("filters by severity", async () => {
    const { user } = renderRoutes(routes, "/alerts", { api: api() });
    await screen.findByRole("grid", { name: "Alerts" });
    await chooseWithKeyboard(user, /Severity/, "Warning");
    await waitFor(() => expect(titles()).toEqual(["Queue filling"]));
    expect(screen.getByText("1 of 2 alerts")).toBeTruthy();
  });

  it("opens the channel an alert concerns", async () => {
    const { user } = renderRoutes(routes, "/alerts", { api: api() });
    await openRow(user, "Queue filling Analyzer COM3 has been disconnected for 10 minutes.");
    expect(await screen.findByText("Navigated to /channels/ch-alpha")).toBeTruthy();
  });

  it("treats no open alerts as a good state", async () => {
    renderRoutes(routes, "/alerts", { api: api({ listAlerts: async () => [] }) });
    expect(await screen.findByText("No open alerts")).toBeTruthy();
  });

  it("shows an error with retry", async () => {
    renderRoutes(routes, "/alerts", {
      api: api({
        listAlerts: async () => {
          throw engineError("engine_unavailable");
        },
      }),
    });
    expect(await screen.findByText("The alerts could not be loaded")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("lists the engine's own alerts with their wording and the page that deals with each", async () => {
    const audit = alert({
      id: "a-audit",
      severity: "critical",
      kind: "audit_unavailable",
      title: "The audit log cannot be written",
      detail: "",
      channelId: null,
      endpointId: null,
      nextAction:
        "Check free disk space and the permissions of the audit directory (<state>/audit). Until the audit log can be written, commands that are audited (configuration changes, channel commands, resends, content views) are refused; message traffic continues.",
    });
    const backup = alert({
      id: "a-backup",
      severity: "warning",
      kind: "backup_stale",
      title: "The newest successful backup is from 2026-11-05T02:00:00Z, more than 26 hours ago",
      detail: "",
      channelId: null,
      endpointId: null,
      nextAction:
        "Check the backup directory (free space, permissions, mounted) and the last backup error (Settings → Backup or `packie backup status`), then run `packie backup create`. The alert clears after the next successful backup.",
    });
    renderRoutes(routes, "/alerts", {
      api: api({ listAlerts: async () => [backup, audit] }),
    });
    const grid = await screen.findByRole("grid", { name: "Alerts" });
    // Critical first, as for every alert; each says what to do in the catalog's words.
    expect(titles()).toEqual([
      "The audit log cannot be written",
      "The newest successful backup is from 2026-11-05T02:00:00Z, more than 26 hours ago",
    ]);
    expect(within(grid).getByText(/Until the audit log can be written/)).toBeTruthy();
    expect(within(grid).getByText(/then run `packie backup create`/)).toBeTruthy();
    expect(within(grid).getByRole("link", { name: "Audit log" }).getAttribute("href")).toBe(
      "/audit",
    );
    expect(within(grid).getByRole("link", { name: "Backup settings" }).getAttribute("href")).toBe(
      "/settings/backup",
    );
  });

  it("links a failing notification target to the Notifications settings", async () => {
    const failing = alert({
      id: "a-notify",
      severity: "warning",
      kind: "notifications_failing",
      title: "Notifications are failing: webhook (connection refused)",
      detail: "targets: webhook, failing since: 2026-11-07T09:00:00Z",
      channelId: null,
      endpointId: null,
      nextAction:
        "Check the notification settings, then send a test message (`packie notify test`).",
    });
    renderRoutes(routes, "/alerts", { api: api({ listAlerts: async () => [failing] }) });
    const grid = await screen.findByRole("grid", { name: "Alerts" });
    expect(
      within(grid).getByRole("link", { name: "Notification settings" }).getAttribute("href"),
    ).toBe("/settings/notifications");
  });
});
