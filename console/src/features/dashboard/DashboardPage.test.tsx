import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import {
  ago,
  alert,
  channel,
  connection,
  engineError,
  failingChannel,
  messageChannel,
  snapshot,
} from "../shared-operate/testFixtures";
import { cellsOf, labels, renderDashboard, rowOf } from "./fixtures";

beforeEach(() => mockGridLayout());

describe("DashboardPage", () => {
  it("shows one table in rank order with the failing channel first", async () => {
    const data = snapshot({
      channels: [
        channel({ id: "ch-a", label: "Alpha bridge", lastActivityAt: ago(500) }),
        messageChannel({ id: "ch-m", label: "Mike messages", state: "retrying" }),
        failingChannel(),
      ],
      node: { ...snapshot().node, state: "degraded" },
    });
    const { container } = renderDashboard(async () => data);

    expect(await screen.findByRole("heading", { level: 1, name: "Dashboard" })).toBeTruthy();
    await screen.findByRole("grid", { name: "Channels" });
    expect(screen.getAllByRole("grid")).toHaveLength(1);
    expect(labels()).toEqual(["Mike messages", "Zulu bridge", "Alpha bridge"]);
    expect(cellsOf(rowOf("Zulu bridge"))[1]?.textContent).toContain("Degraded");
    await expectNoA11yViolations(container);
  });

  it("shows the header strip: engine, version, uptime, configuration, counts, alerts, deliveries, store, connection", async () => {
    const base = snapshot();
    const data = snapshot({
      node: {
        ...base.node,
        state: "degraded",
        pendingConfigVersion: 8,
        totals: {
          deliveriesUnresolved: 3,
          deliveriesQueued: 4,
          alertsWarning: 1,
          alertsCritical: 2,
        },
      },
      channels: [
        channel({ id: "a", label: "A" }),
        channel({ id: "b", label: "B" }),
        channel({ id: "c", label: "C", state: "retrying" }),
        channel({ id: "d", label: "D", state: "stopped", stopReason: "paused" }),
      ],
    });
    renderDashboard(async () => data);
    const strip = await screen.findByRole("region", { name: "Engine" });

    expect(strip.textContent).toContain("Needs attention");
    expect(strip.textContent).toContain("0.1.0");
    expect(strip.textContent).toContain("1 d 1 h");
    expect(strip.textContent).toContain("Version 7");
    expect(within(strip).getByText("Applying version 8")).toBeTruthy();
    const counts = within(within(strip).getByRole("list", { name: "Channels by state" }));
    expect(counts.getAllByRole("listitem").map((item) => item.textContent)).toEqual([
      "2 Running",
      "1 Retrying",
      "1 Stopped",
    ]);
    expect(within(strip).getByText("2 critical")).toBeTruthy();
    expect(within(strip).getByText("1 warning")).toBeTruthy();
    expect(within(strip).getByText("3 need attention")).toBeTruthy();
    expect(within(strip).getByRole("link", { name: "View in Messages" }).getAttribute("href")).toBe(
      "/messages?quick=attention&range=any",
    );
    expect(strip.textContent).toContain("1,234 messages · 5.0 MB");
    expect(strip.textContent).toContain("50.0 GB of 100.0 GB (50%)");
    await vi.waitFor(() => expect(strip.textContent).toContain("Connected"));
  });

  it("omits what the engine does not report and falls back to counting open alerts", async () => {
    const base = snapshot();
    renderDashboard(async () =>
      snapshot({
        node: { ...base.node, store: null, totals: null },
        openAlerts: [alert({ severity: "warning", channelId: "ch-alpha" })],
      }),
    );
    const strip = await screen.findByRole("region", { name: "Engine" });
    expect(strip.textContent).not.toContain("Disk free");
    expect(within(strip).getByText("1 warning")).toBeTruthy();
    expect(within(strip).queryByText(/critical/)).toBeNull();
  });

  it("dims the last known state and says so while reconnecting", async () => {
    const { container } = renderDashboard(
      async () => snapshot(),
      async () => connection("reconnecting", "Waiting for the engine to answer."),
    );
    expect(await screen.findByText("Reconnecting")).toBeTruthy();
    expect(screen.getByText("The last known state is shown dimmed.")).toBeTruthy();
    expect(container.querySelector("[data-stale]")).not.toBeNull();
  });

  it("opens the channel page from a row", async () => {
    const { user } = renderDashboard(async () => snapshot());
    await openRow(user, "Alpha bridge Bridge");
    expect(await screen.findByText("Navigated to /channels/ch-alpha")).toBeTruthy();
  });

  it("lists open alerts and endpoints below the table", async () => {
    renderDashboard(async () => snapshot({ openAlerts: [alert()] }));
    const section = (await screen.findByRole("heading", { name: "Open alerts" })).closest(
      "section",
    ) as HTMLElement;
    expect(within(section).getByText("Endpoint down")).toBeTruthy();
    expect(within(section).getByText(/Check the serial cable/)).toBeTruthy();
    expect(within(section).getByRole("link", { name: "View alerts" }).getAttribute("href")).toBe(
      "/alerts",
    );
    expect(screen.getByRole("heading", { name: "Endpoints" })).toBeTruthy();
  });

  it("offers to add the first channel when there are none", async () => {
    renderDashboard(async () => snapshot({ channels: [], endpoints: [] }));
    expect(await screen.findByText("No channels yet")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Add channel" }).getAttribute("href")).toBe(
      "/channels/new",
    );
  });

  it("shows the heading while loading", async () => {
    renderDashboard(() => new Promise(() => undefined));
    expect(screen.getByRole("heading", { level: 1, name: "Dashboard" })).toBeTruthy();
    expect(await screen.findByText("Loading dashboard…")).toBeTruthy();
  });

  it("shows the error with a retry that asks again", async () => {
    const getSnapshot = vi
      .fn()
      .mockRejectedValueOnce(engineError("engine_unavailable", "The engine is not running."))
      .mockResolvedValue(snapshot());
    const { user } = renderDashboard(getSnapshot);
    expect(await screen.findByText("The dashboard could not be loaded")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("grid", { name: "Channels" })).toBeTruthy();
    expect(getSnapshot).toHaveBeenCalledTimes(2);
  });

  it("says plainly that audited actions are refused while the audit log cannot be written", async () => {
    const data = snapshot({
      openAlerts: [
        alert({
          id: "a-audit",
          kind: "audit_unavailable",
          title: "The audit log cannot be written",
          channelId: null,
          endpointId: null,
        }),
      ],
    });
    renderDashboard(async () => data);
    const banner = await screen.findByText(/are refused until it can be written again/);
    expect(banner).toBeTruthy();
    expect(
      screen.getAllByRole("link", { name: "Open the audit log" })[0]?.getAttribute("href"),
    ).toBe("/audit");
    // The alert is also in the list of open alerts, linking to the same page.
    expect(
      within(screen.getByRole("list", { name: "Open alerts" })).getByRole("link", {
        name: "Audit log",
      }),
    ).toBeTruthy();
  });

  it("links a failing notification target to the Notifications settings", async () => {
    renderDashboard(async () =>
      snapshot({
        openAlerts: [
          alert({
            id: "a-notify",
            severity: "warning",
            kind: "notifications_failing",
            title: "Notifications are failing: webhook (connection refused)",
            channelId: null,
            endpointId: null,
          }),
        ],
      }),
    );
    const list = await screen.findByRole("list", { name: "Open alerts" });
    expect(
      within(list).getByRole("link", { name: "Notification settings" }).getAttribute("href"),
    ).toBe("/settings/notifications");
  });

  it("shows no audit banner while the audit log works", async () => {
    renderDashboard(async () => snapshot());
    await screen.findByRole("grid", { name: "Channels" });
    expect(screen.queryByText(/are refused until it can be written again/)).toBeNull();
  });
});
