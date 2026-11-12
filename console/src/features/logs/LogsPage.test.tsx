import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { columnTexts, gridRows, openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { status } from "@/lib/test/liveStatus";
import { renderRoutes } from "@/lib/test/router";
import { chooseOption } from "@/lib/test/select";
import { event, newestFirst, page, type StubApi, stubApi } from "./fixtures";
import { LogsPage } from "./LogsPage";

const ROUTES = [{ path: "/logs", element: <LogsPage /> }];

/** The messages of the rows shown, top to bottom. */
const shown = () => columnTexts(5);

async function open(api: StubApi, path = "/logs") {
  const rendered = renderRoutes(ROUTES, path, { api });
  await screen.findByRole("grid", { name: "Log events" });
  // The subscription starts with the first render, before the rows arrive.
  return rendered;
}

/** Scrolls the grid to `top`, as the operator's wheel would. */
function scrollTo(top: number) {
  const grid = screen.getByRole("grid", { name: "Log events" });
  act(() => {
    grid.scrollTop = top;
    fireEvent.scroll(grid);
  });
}

describe("LogsPage", () => {
  beforeEach(mockGridLayout);

  it("shows the newest events first, in the default columns, and has no violations", async () => {
    const api = stubApi();
    const { container } = await open(api);
    expect(
      within(screen.getByRole("grid"))
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["Time", "Level", "Event", "Channel", "Endpoint", "Message"]);
    expect(shown()).toEqual(["Synthetic event 3", "Synthetic event 2", "Synthetic event 1"]);
    const first = within(screen.getAllByRole("row")[1] as HTMLElement);
    expect(first.getByText("Synthetic chemistry")).toBeTruthy();
    expect(first.getByText("Synthetic LIS port")).toBeTruthy();
    expect(first.getByText("Info")).toBeTruthy();
    expect(first.getByText("2026-11-06 12:00:03.000")).toBeTruthy();
    expect(status("3 loaded")).toBe(true);
    await expectNoA11yViolations(container);
  });

  it("asks for the newest 200 at Info and above, and follows with the same filters", async () => {
    const api = stubApi();
    await open(api);
    expect(api.queryLogs).toHaveBeenCalledWith({ minLevel: "info" }, { before: null, limit: 200 });
    expect(api.follows).toHaveLength(1);
    expect(api.follows[0]?.query).toEqual({ minLevel: "info" });
    expect(status("Following")).toBe(true);
  });

  it("puts events that arrive at the top, newest first, without duplicating those already read", async () => {
    const api = stubApi();
    await open(api);
    act(() => api.follows[0]?.push([event(3), event(4), event(5)]));
    expect(shown()).toEqual([
      "Synthetic event 5",
      "Synthetic event 4",
      "Synthetic event 3",
      "Synthetic event 2",
      "Synthetic event 1",
    ]);
    expect(status("5 loaded")).toBe(true);
    act(() => api.follows[0]?.push([event(6)]));
    expect(shown()[0]).toBe("Synthetic event 6");
  });

  it("holds the list when the operator scrolls away, counts what arrives, and shows it on Resume", async () => {
    const api = stubApi();
    const { user } = await open(api);
    scrollTo(300);
    expect(status("Paused — 0 new events")).toBe(true);
    act(() => api.follows[0]?.push([event(4), event(5)]));
    expect(status("Paused — 2 new events")).toBe(true);
    expect(shown()).toEqual(["Synthetic event 3", "Synthetic event 2", "Synthetic event 1"]);
    act(() => api.follows[0]?.push([event(6)]));
    expect(status("Paused — 3 new events")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Jump to live" }));
    expect(shown().slice(0, 4)).toEqual([
      "Synthetic event 6",
      "Synthetic event 5",
      "Synthetic event 4",
      "Synthetic event 3",
    ]);
    expect(status("Following")).toBe(true);
    expect(screen.getByRole("grid").scrollTop).toBe(0);
  });

  it("resumes by itself when the operator scrolls back to the top", async () => {
    const api = stubApi();
    await open(api);
    scrollTo(300);
    act(() => api.follows[0]?.push([event(4)]));
    expect(shown()[0]).toBe("Synthetic event 3");
    scrollTo(0);
    expect(status("Following")).toBe(true);
    expect(shown()[0]).toBe("Synthetic event 4");
  });

  it("holds the list on an explicit Pause until Resume, even when scrolled back to the top", async () => {
    const api = stubApi();
    const { user } = await open(api);
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(status("Paused — 0 new events")).toBe(true);
    act(() => api.follows[0]?.push([event(4)]));
    expect(status("Paused — 1 new event")).toBe(true);
    scrollTo(300);
    scrollTo(0);
    expect(status("Paused — 1 new event")).toBe(true);
    expect(shown()[0]).toBe("Synthetic event 3");
    await user.click(screen.getByRole("button", { name: "Jump to live" }));
    expect(shown()[0]).toBe("Synthetic event 4");
  });

  it("opens an event with all its fields, holds the list while open, and resumes when closed", async () => {
    const api = stubApi({
      queryLogs: async () =>
        page([
          event(2, {
            level: "error",
            message: "Delivery failed: connection refused",
            messageId: "msg-000009",
            fields: { attempt: "3", destination: "dest-lis" },
          }),
          event(1),
        ]),
    });
    const { user } = await open(api);
    await openRow(user, "Delivery failed: connection refused");
    const panel = screen.getByRole("region", { name: "Event 2" });
    expect(within(panel).getByText("Delivery failed: connection refused")).toBeTruthy();
    expect(within(panel).getByText("Error")).toBeTruthy();
    expect(within(panel).getByText("attempt")).toBeTruthy();
    expect(within(panel).getByText("dest-lis")).toBeTruthy();
    expect(within(panel).getByRole("link", { name: "msg-000009" }).getAttribute("href")).toBe(
      "/messages/msg-000009",
    );
    expect(
      within(panel).getByRole("link", { name: "Synthetic chemistry" }).getAttribute("href"),
    ).toBe("/channels/ch-chem");
    expect(within(panel).getByText("2026-11-06 12:00:02.000 UTC", { exact: false })).toBeTruthy();
    expect(status("Paused")).toBe(true);
    act(() => api.follows[0]?.push([event(3)]));
    expect(shown()[0]).toBe("Delivery failed: connection refused");
    await user.click(within(panel).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("region", { name: "Event 2" })).toBeNull();
    expect(status("Following")).toBe(true);
    expect(shown()[0]).toBe("Synthetic event 3");
  });

  it("closes the event with Escape and steps to the next older and newer events", async () => {
    const api = stubApi();
    const { user } = await open(api);
    await openRow(user, "Synthetic event 3");
    const panel = screen.getByRole("region", { name: "Event 3" });
    expect(within(panel).getByRole("button", { name: "Previous" }).hasAttribute("disabled")).toBe(
      true,
    );
    await user.click(within(panel).getByRole("button", { name: "Next" }));
    expect(screen.getByRole("region", { name: "Event 2" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Previous" }));
    expect(screen.getByRole("region", { name: "Event 3" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("region", { name: "Event 3" })).toBeNull();
    await openRow(user, "Synthetic event 1");
    const again = screen.getByRole("region", { name: "Event 1" });
    act(() => within(again).getByRole("button", { name: "Previous" }).focus());
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("region", { name: "Event 1" })).toBeNull();
  });

  it("changes the query and restarts following when the level changes, keeping it in the URL", async () => {
    const api = stubApi();
    const { user, router } = await open(api);
    const firstFollow = api.follows[0];
    await chooseOption(user, /Level/, "Warning and above");
    await waitFor(() =>
      expect(api.queryLogs).toHaveBeenLastCalledWith(
        { minLevel: "warn" },
        { before: null, limit: 200 },
      ),
    );
    expect(router.state.location.search).toBe("?level=warn");
    expect(firstFollow?.stop).toHaveBeenCalled();
    await waitFor(() => expect(api.follows.at(-1)?.query).toEqual({ minLevel: "warn" }));
    // Events followed for the old filter never show under the new one.
    act(() => firstFollow?.push([event(9, { message: "Old filter event" })]));
    expect(screen.queryByText("Old filter event")).toBeNull();
  });

  it("starts from the filters in the URL, as the channel and endpoint pages link them", async () => {
    const api = stubApi();
    const before = Date.now();
    await open(api, "/logs?level=error&channel=ch-chem&endpoint=ep-lis&text=refused&range=1h");
    const [query] = api.queryLogs.mock.calls[0] ?? [];
    expect(query).toMatchObject({
      minLevel: "error",
      channelId: "ch-chem",
      endpointId: "ep-lis",
      text: "refused",
    });
    const from = Date.parse(query?.from ?? "");
    expect(from).toBeGreaterThanOrEqual(before - 3_600_000);
    expect(from).toBeLessThanOrEqual(Date.now() - 3_600_000);
    // A look-back range is not followed unless the operator asks.
    expect(api.follows).toHaveLength(0);
    expect(status("Not following")).toBe(true);
  });

  it("follows a look-back range once the operator asks, reading the list again first", async () => {
    const api = stubApi();
    const { user } = await open(api, "/logs?range=24h");
    expect(api.queryLogs).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Follow" }));
    await waitFor(() => expect(api.follows).toHaveLength(1));
    await waitFor(() => expect(api.queryLogs).toHaveBeenCalledTimes(2));
    expect(status("Following")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Follow" }));
    expect(api.follows[0]?.stop).toHaveBeenCalled();
    expect(status("Not following")).toBe(true);
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
  });

  it("cannot follow a range that ends in the past, and says so", async () => {
    const api = stubApi();
    await open(api, "/logs?range=custom&from=2026-11-01T00:00:00Z&to=2026-11-02T00:00:00Z");
    expect(api.follows).toHaveLength(0);
    expect(status("Not following: the range ends in the past")).toBe(true);
    expect(screen.getByRole("button", { name: "Follow" }).hasAttribute("disabled")).toBe(true);
  });

  it("loads older events from the cursor and says when there are no more", async () => {
    const api = stubApi({
      queryLogs: async (_query: unknown, { before }: { before: number | null }) =>
        before === null ? page(newestFirst(10, 8), 8) : page(newestFirst(7, 6), null),
    });
    const { user } = await open(api);
    expect(status("3 loaded · older events available")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Load older events" }));
    await waitFor(() => expect(status("5 loaded")).toBe(true));
    expect(api.queryLogs).toHaveBeenLastCalledWith({ minLevel: "info" }, { before: 8, limit: 200 });
    expect(shown().slice(-2)).toEqual(["Synthetic event 7", "Synthetic event 6"]);
    expect(screen.queryByRole("button", { name: "Load older events" })).toBeNull();
    expect(screen.getByText("No older events in the engine's log.")).toBeTruthy();
    expect(status("older events available")).toBe(false);
    expect(status("5 loaded · no older events in the engine's log")).toBe(true);
  });

  it("shows why older events could not be loaded, and keeps the ones shown", async () => {
    const api = stubApi({
      queryLogs: async (_query: unknown, { before }: { before: number | null }) => {
        if (before === null) return page(newestFirst(3, 1), 1);
        throw engineError("internal", "The log buffer is busy.");
      },
    });
    const { user } = await open(api);
    await user.click(screen.getByRole("button", { name: "Load older events" }));
    expect(await screen.findByText("Older events could not be loaded")).toBeTruthy();
    expect(screen.getByText("The log buffer is busy.")).toBeTruthy();
    expect(shown()).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Load older events" })).toBeTruthy();
  });

  it("keeps a bounded window of the newest 10,000 rows, and says so", async () => {
    const api = stubApi({ queryLogs: async () => page(newestFirst(3, 1), 1) });
    const { router, user } = await open(api);
    expect(status("Following · newest 10,000 rows kept")).toBe(true);
    act(() => api.follows[0]?.push(newestFirst(10_203, 4).reverse()));
    expect(status("10,000 loaded · window full")).toBe(true);
    expect(shown()[0]).toBe("Synthetic event 10203");
    expect(screen.queryByRole("button", { name: "Load older events" })).toBeNull();
    expect(screen.getByText(/The newest 10,000 rows are kept/)).toBeTruthy();
    expect(screen.getByRole("grid").getAttribute("aria-rowcount")).toBe("10001");
    // Older events are asked for by time: a range that ends before the oldest one shown.
    const oldest = event(10_203 - 9_999).at;
    await user.click(screen.getByRole("button", { name: /Show events before/ }));
    await waitFor(() =>
      expect(router.state.location.search).toBe(`?range=custom&to=${encodeURIComponent(oldest)}`),
    );
    expect(api.queryLogs).toHaveBeenLastCalledWith(
      { minLevel: "info", to: oldest },
      { before: null, limit: 200 },
    );
    expect(status("Not following: the range ends in the past")).toBe(true);
  });

  it("says when nothing has been logged, and when the filters exclude everything", async () => {
    const api = stubApi({ queryLogs: async () => page([]) });
    renderRoutes(ROUTES, "/logs", { api });
    expect(await screen.findByText("No log events yet")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
    // An event can still arrive while following.
    act(() => api.follows[0]?.push([event(1)]));
    expect(await screen.findByRole("grid")).toBeTruthy();
  });

  it("offers to clear filters that exclude everything", async () => {
    const api = stubApi({
      queryLogs: async (query: { minLevel?: string }) =>
        query.minLevel === "error" ? page([]) : page(newestFirst(2, 1)),
    });
    const { user, router } = renderRoutes(ROUTES, "/logs?level=error", { api });
    expect(await screen.findByText("No events match the filters")).toBeTruthy();
    await user.click(screen.getAllByRole("button", { name: "Clear filters" })[1] as HTMLElement);
    expect(await screen.findByRole("grid")).toBeTruthy();
    expect(router.state.location.search).toBe("");
  });

  it("shows the error with a retry, keeping the filters usable", async () => {
    let fail = true;
    const api = stubApi({
      queryLogs: async () => {
        if (fail) throw engineError("engine_unavailable", "The engine is not running.");
        return page(newestFirst(2, 1));
      },
    });
    const { user } = renderRoutes(ROUTES, "/logs", { api });
    expect(await screen.findByText("The log events could not be loaded")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Level/ })).toBeTruthy();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("grid")).toBeTruthy();
  });

  it("names channels and endpoints by label, and marks events that name none", async () => {
    const api = stubApi({
      queryLogs: async () =>
        page([event(2, { channelId: null, endpointId: "ep-gone" }), event(1, { level: "warn" })]),
    });
    await open(api);
    const [engineWide, warned] = gridRows() as HTMLElement[];
    expect(within(engineWide as HTMLElement).getByText("None")).toBeTruthy();
    expect(within(engineWide as HTMLElement).getByText("ep-gone")).toBeTruthy();
    expect(within(warned as HTMLElement).getByText("Warning")).toBeTruthy();
  });
});
