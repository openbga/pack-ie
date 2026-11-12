import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useConnectionSync } from "@/api/hooks";
import { MockEngineApi } from "@/api/mock";
import type { LogFollowNotice } from "@/api/types";
import { columnTexts, gridRows, openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { status } from "@/lib/test/liveStatus";
import { renderRoutes } from "@/lib/test/router";
import { event, newestFirst, page, stubApi } from "./fixtures";
import { LogsPage } from "./LogsPage";

/** The Logs page with the connection state kept current, as the app's shell does. */
function Synced() {
  useConnectionSync();
  return <LogsPage />;
}

const ROUTES = [{ path: "/logs", element: <Synced /> }];

const cells = (row: HTMLElement) => within(row).getAllByRole("gridcell");

describe("Logs: the event column and filter", () => {
  beforeEach(mockGridLayout);

  it("shows each event's catalog name in its own column, empty for an event without one", async () => {
    const api = stubApi({
      queryLogs: async () =>
        page([event(2, { event: "delivery.state_changed" }), event(1, { event: null })]),
    });
    renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    const [named, unnamed] = gridRows();
    expect(cells(named as HTMLElement)[2]?.textContent).toBe("delivery.state_changed");
    expect(cells(unnamed as HTMLElement)[2]?.textContent).toBe("");
  });

  it("asks the engine for the event in the URL, and follows with it", async () => {
    const api = stubApi();
    renderRoutes(ROUTES, "/logs?event=channel.started", { api });
    await screen.findByRole("grid", { name: "Log events" });
    expect(api.queryLogs).toHaveBeenCalledWith(
      { minLevel: "info", event: "channel.started" },
      { before: null, limit: 200 },
    );
    expect(api.follows[0]?.query).toEqual({ minLevel: "info", event: "channel.started" });
    expect(screen.getByRole("combobox", { name: "Event" })).toHaveProperty(
      "value",
      "channel.started",
    );
  });

  it("puts a typed event in the URL and queries again", async () => {
    const api = stubApi();
    const { user, router } = renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    await user.type(
      screen.getByRole("combobox", { name: "Event" }),
      "delivery.state_changed{Enter}",
    );
    await waitFor(() => expect(router.state.location.search).toBe("?event=delivery.state_changed"));
    await waitFor(() =>
      expect(api.queryLogs).toHaveBeenLastCalledWith(
        { minLevel: "info", event: "delivery.state_changed" },
        { before: null, limit: 200 },
      ),
    );
  });

  it("says that the engine matches whole names when an unknown one shows nothing", async () => {
    const api = stubApi({ queryLogs: async () => page([]) });
    renderRoutes(ROUTES, "/logs?event=delivery.", { api });
    expect(await screen.findByText("No events match the filters")).toBeTruthy();
    expect(screen.getByText(/matches the whole event name/)).toBeTruthy();
  });
});

describe("Logs: an event's details", () => {
  beforeEach(mockGridLayout);

  it("shows event, target, sequence, message and fields, linking channel, endpoint and message", async () => {
    const api = stubApi({
      queryLogs: async () =>
        page([
          event(7, {
            event: "delivery.state_changed",
            target: "packie_routing::message",
            messageId: "msg-000007",
            fields: { delivery_id: "d-1", ack_code: "AA" },
          }),
        ]),
    });
    const { user } = renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    await openRow(user, "Synthetic event 7");
    const panel = screen.getByRole("region", { name: "Event 7" });
    expect(within(panel).getByText("delivery.state_changed")).toBeTruthy();
    expect(within(panel).getByText("packie_routing::message")).toBeTruthy();
    expect(within(panel).getByText("ack_code")).toBeTruthy();
    expect(within(panel).getByText("d-1")).toBeTruthy();
    const link = (name: string) => within(panel).getByRole("link", { name }).getAttribute("href");
    expect(link("Synthetic chemistry")).toBe("/channels/ch-chem");
    expect(link("Synthetic LIS port")).toBe("/endpoints/ep-lis");
    expect(link("msg-000007")).toBe("/messages/msg-000007");
  });

  it("links ids the snapshot does not know by the id itself", async () => {
    const api = stubApi({
      queryLogs: async () => page([event(1, { channelId: "ch-gone", endpointId: "ep-gone" })]),
    });
    const { user } = renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    await openRow(user, "Synthetic event 1");
    const panel = screen.getByRole("region", { name: "Event 1" });
    expect(within(panel).getByRole("link", { name: "ch-gone" }).getAttribute("href")).toBe(
      "/channels/ch-gone",
    );
    expect(within(panel).getByRole("link", { name: "ep-gone" }).getAttribute("href")).toBe(
      "/endpoints/ep-gone",
    );
  });
});

describe("Logs: dividers for what an outage cost", () => {
  beforeEach(mockGridLayout);

  const notice = (extra: Partial<LogFollowNotice> = {}): LogFollowNotice => ({
    lostAt: "2026-11-06T12:03:04Z",
    backAt: "2026-11-06T12:03:09Z",
    filled: 2,
    missing: "none",
    ...extra,
  });
  const dividers = () =>
    gridRows()
      .map((row) => cells(row)[5]?.textContent ?? "")
      .filter((text) => text.startsWith("Connection lost"));

  async function open() {
    const api = stubApi({ queryLogs: async () => page(newestFirst(3, 1)) });
    renderRoutes(ROUTES, "/logs?level=info", { api });
    await screen.findByRole("grid", { name: "Log events" });
    return api;
  }

  it("shows the events filled in above a divider with the times and the count", async () => {
    const api = await open();
    act(() => api.follows[0]?.push([event(4), event(5)], notice()));
    expect(columnTexts(5)).toEqual([
      "Synthetic event 5",
      "Synthetic event 4",
      expect.stringMatching(
        /^Connection lost \d\d:\d\d:04 — reconnected \d\d:\d\d:09, 2 events filled in\.$/,
      ),
      "Synthetic event 3",
      "Synthetic event 2",
      "Synthetic event 1",
    ]);
    expect(status("5 loaded")).toBe(true);
  });

  it("says when nothing was missed, and when one event was", async () => {
    const api = await open();
    act(() => api.follows[0]?.push([], notice({ filled: 0 })));
    act(() => api.follows[0]?.push([event(4)], notice({ filled: 1 })));
    expect(dividers()[0]).toMatch(/, 1 event filled in\.$/);
    expect(dividers()[1]).toMatch(/, no events were missed\.$/);
  });

  it("says that some events of the outage are no longer available", async () => {
    const api = await open();
    act(() => api.follows[0]?.push([event(40)], notice({ filled: 1, missing: "gap" })));
    expect(dividers()[0]).toMatch(
      /1 event filled in\. Some events during the outage are no longer available\.$/,
    );
  });

  it("says the engine restarted, keeps the earlier events below, and starts the sequence over", async () => {
    const api = await open();
    act(() => api.follows[0]?.push([event(1), event(2)], notice({ missing: "restarted" })));
    expect(dividers()[0]).toMatch(
      /reconnected \d\d:\d\d:09\. Engine restarted — earlier events are no longer available\.$/,
    );
    // The new run's events carry seq 1 and 2 again: shown above the divider, the old ones kept.
    expect(gridRows().map((row) => cells(row)[5]?.textContent?.slice(0, 18))).toEqual([
      "Synthetic event 2",
      "Synthetic event 1",
      "Connection lost 12",
      "Synthetic event 3",
      "Synthetic event 2",
      "Synthetic event 1",
    ]);
    act(() => api.follows[0]?.push([event(2), event(3)]));
    expect(gridRows()[0] && cells(gridRows()[0] as HTMLElement)[5]?.textContent).toBe(
      "Synthetic event 3",
    );
    expect(gridRows()).toHaveLength(7);
  });

  it("cannot load older events of a run that has ended", async () => {
    const api = stubApi({ queryLogs: async () => page(newestFirst(3, 1), 1) });
    renderRoutes(ROUTES, "/logs", { api });
    await screen.findByRole("grid", { name: "Log events" });
    expect(screen.getByRole("button", { name: "Load older events" })).toBeTruthy();
    act(() => api.follows[0]?.push([event(1)], notice({ missing: "restarted" })));
    expect(screen.queryByRole("button", { name: "Load older events" })).toBeNull();
    expect(
      screen.getByText(/The engine restarted: its earlier events are no longer available/),
    ).toBeTruthy();
  });

  it("does not open a divider like an event", async () => {
    const api = await open();
    act(() => api.follows[0]?.push([event(4)], notice()));
    const divider = gridRows().find((row) =>
      cells(row)[5]?.textContent?.startsWith("Connection lost"),
    );
    act(() => divider?.click());
    expect(screen.queryByRole("region", { name: /^Event / })).toBeNull();
  });
});

describe("Logs against an engine that goes away", () => {
  let api: MockEngineApi;
  beforeEach(() => {
    mockGridLayout();
    api = new MockEngineApi({ latency: "none", messageCount: 50, logRingSize: 300 });
  });
  afterEach(() => api.dispose());

  const loaded = () =>
    Number(
      /^([\d,]+) loaded/.exec(screen.getByText(/ loaded/).textContent ?? "")?.[1]?.replace(",", ""),
    );
  const dividerText = () =>
    screen.queryByText(/^Connection lost/)?.textContent ??
    screen.queryByText(/Engine restarted/)?.textContent;

  // Few rows match, so the divider is among the rows the (virtualized) grid draws.
  async function open(level = "error") {
    renderRoutes(ROUTES, `/logs?level=${level}`, { api });
    await screen.findByRole("grid", { name: "Log events" });
    await waitFor(() => expect(status("Following")).toBe(true));
  }

  it("waits for the engine, then fills in what it logged meanwhile once, behind a divider", async () => {
    await open("trace");
    const before = loaded();
    act(() => api.simulateOutage());
    await waitFor(() =>
      expect(status("Waiting for the engine… following resumes automatically")).toBe(true),
    );
    act(() => api.publishLogEvents(5));
    expect(loaded()).toBe(before);
    expect(screen.queryByText(/^Connection lost/)).toBeNull();
    act(() => api.endOutage());
    await waitFor(() => expect(status("Following")).toBe(true));
    expect(dividerText()).toMatch(/reconnected \d\d:\d\d:\d\d, \d+ events filled in\.$/);
    expect(loaded()).toBeGreaterThanOrEqual(before + 5);
  });

  it("says the engine restarted, with the new run's events above the divider", async () => {
    await open("info");
    act(() => api.simulateOutage());
    act(() => api.endOutage({ restarted: true }));
    expect(
      await screen.findByText(/Engine restarted — earlier events are no longer available\./),
    ).toBeTruthy();
    expect(screen.getAllByText("node.started").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Load older events" })).toBeNull();
  });

  it("says that events were lost when the ring dropped some of the outage", async () => {
    await open();
    act(() => api.simulateOutage());
    act(() => api.publishLogEvents(400));
    act(() => api.endOutage());
    expect(
      await screen.findByText(/Some events during the outage are no longer available\./),
    ).toBeTruthy();
  });
});
