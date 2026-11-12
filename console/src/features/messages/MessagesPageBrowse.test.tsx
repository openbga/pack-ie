import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { MessageQuery } from "@/api/types";
import { engineError } from "@/lib/test/engineError";
import { gridRows, openRow, openRowActions, openRowWithEnter } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { status } from "@/lib/test/liveStatus";
import { renderRoutes } from "@/lib/test/router";
import { channel, chip, detail, page, rows, snapshot, stubApi, summary } from "./fixtures";
import { lastQuery, ROUTES } from "./messagesListSetup";
import { forgetMessagesSession } from "./messagesSession";

describe("MessagesPage", () => {
  beforeEach(() => {
    mockGridLayout();
    forgetMessagesSession();
  });

  it("keeps the rows on screen, marked busy, while the next page loads", async () => {
    let release: (answer: ReturnType<typeof page>) => void = () => undefined;
    const api = stubApi({
      listMessages: async (query: { offset: number; limit: number }) => {
        if (query.limit === 1) return page(rows(1, 1), 0);
        return query.offset === 0
          ? page(rows(1, 100), 250)
          : new Promise((resolve) => {
              release = resolve;
            });
      },
    });
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    const grid = await screen.findByRole("grid");
    await user.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(grid.getAttribute("aria-busy")).toBe("true"));
    expect(screen.getByRole("gridcell", { name: "CTRL-000001" })).toBeTruthy();
    expect(status("Updating")).toBe(true);
    release(page(rows(101, 100), 250));
    await waitFor(() => expect(grid.hasAttribute("aria-busy")).toBe(false));
    expect(screen.getByRole("gridcell", { name: "CTRL-000101" })).toBeTruthy();
    expect(screen.queryByRole("gridcell", { name: "CTRL-000001" })).toBeNull();
  });

  it("sorts on the engine when a header is clicked, from the first page", async () => {
    const api = stubApi();
    const { user, router } = renderRoutes(ROUTES, "/messages?page=2", { api });
    await screen.findByRole("grid");
    expect(screen.getByRole("columnheader", { name: "Received" }).getAttribute("aria-sort")).toBe(
      "descending",
    );
    await user.click(screen.getByRole("columnheader", { name: "Channel" }));
    await waitFor(() =>
      expect(lastQuery(api)).toMatchObject({
        sort: { field: "channel", descending: false },
        offset: 0,
      }),
    );
    expect(router.state.location.search).toBe("?sort=channel");
    await user.click(screen.getByRole("columnheader", { name: "Channel" }));
    await waitFor(() => expect(router.state.location.search).toBe("?sort=channel&desc=1"));
    // The engine sorts on five columns only.
    for (const name of ["Direction", "Delivery summary", "Identifier", "Patient ID"]) {
      expect(screen.getByRole("columnheader", { name }).hasAttribute("aria-sort")).toBe(false);
    }
  });

  it("filters from the quick filters, from the first page, and clears them again", async () => {
    const api = stubApi();
    const { user, router } = renderRoutes(ROUTES, "/messages?page=2", { api });
    await screen.findByRole("grid");
    await user.click(screen.getByRole("button", { name: /^Needs attention/ }));
    await waitFor(() =>
      expect(lastQuery(api)).toMatchObject({
        filter: { deliverySummaries: ["attention"] },
        offset: 0,
      }),
    );
    expect(router.state.location.search).toBe("?quick=attention");
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(router.state.location.search).toBe(""));
    expect(lastQuery(api)?.filter.deliverySummaries).toBeUndefined();
  });

  it("shows the quick filters' counts, which the engine gives as the total of a one-row query", async () => {
    const api = stubApi({
      listMessages: async (query: MessageQuery) =>
        query.limit === 1
          ? page(rows(1, 1), query.filter.states?.includes("duplicate") ? 7 : 2)
          : page(rows(1, 3)),
    });
    renderRoutes(ROUTES, "/messages", { api });
    expect(await screen.findByRole("button", { name: "Duplicates 7" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Needs attention 2" })).toBeTruthy();
    const counts = api.listMessages.mock.calls.map(([query]) => query).filter((q) => q.limit === 1);
    expect(counts.length).toBeGreaterThanOrEqual(4);
  });

  it("offers the channels whose messages are stored: message channels and recording bridges", async () => {
    const recording = channel("br-rec", "Recording bridge", "bridge");
    recording.bridge = {
      sessionState: "bridged",
      connectStrategy: "parallel",
      receiver: null,
      endpointA: {} as never,
      endpointB: {} as never,
      bytesAToB: 0,
      bytesBToA: 0,
      messagesAToB: 0,
      messagesBToA: 0,
      framingErrors: 0,
      tapDropped: 0,
      overflowCount: 0,
      tap: { framer: "astm", recordMessages: true, transcriptViewers: 0 },
    };
    const api = stubApi({
      getSnapshot: async () => {
        const base = snapshot();
        return { ...base, channels: [...base.channels, recording] };
      },
    });
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.click(screen.getByRole("button", { name: /Channels/ }));
    const dialog = await screen.findByRole("dialog", { name: "Channels" });
    expect(
      within(dialog)
        .getAllByRole("checkbox")
        .map((box) => box.closest("label")?.textContent),
    ).toEqual(["Synthetic chemistry", "Synthetic hematology", "Recording bridge"]);
  });

  describe("when the list is empty", () => {
    it("says nothing is stored yet when the store holds no message", async () => {
      const api = stubApi({
        getSnapshot: async () =>
          snapshot({
            state: "ok",
            messageCount: 0,
            databaseBytes: 0,
            diskFreeBytes: 1,
            diskTotalBytes: 2,
          }),
        listMessages: async () => page([]),
      });
      renderRoutes(ROUTES, "/messages", { api });
      expect(await screen.findByText("No messages yet")).toBeTruthy();
      expect(screen.queryByText("No messages match")).toBeNull();
      expect(screen.queryByRole("grid")).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("says nothing is stored yet for any time and no filter, even when the count is unknown", async () => {
      const api = stubApi({
        getSnapshot: async () => snapshot(null),
        listMessages: async () => page([]),
      });
      renderRoutes(ROUTES, "/messages?range=any", { api });
      expect(await screen.findByText("No messages yet")).toBeTruthy();
    });

    it("says messages exist outside the time range, and offers any time", async () => {
      const api = stubApi({
        listMessages: async (query: MessageQuery) =>
          query.filter.since === undefined ? page(rows(1, 2)) : page([]),
      });
      const { user, router } = renderRoutes(ROUTES, "/messages", { api });
      expect(await screen.findByText("No messages in this time range")).toBeTruthy();
      expect(screen.queryByText("No messages yet")).toBeNull();
      await user.click(screen.getByRole("button", { name: "Show any time" }));
      expect(await screen.findByRole("grid")).toBeTruthy();
      expect(router.state.location.search).toBe("?range=any");
    });

    it("says no message matches the filters, and offers to clear them", async () => {
      const api = stubApi({
        listMessages: async (query: MessageQuery) =>
          query.filter.states ? page([]) : page(rows(1, 2)),
      });
      const { user } = renderRoutes(ROUTES, "/messages?state=invalid", { api });
      expect(await screen.findByText("No messages match")).toBeTruthy();
      expect(screen.queryByText("No messages yet")).toBeNull();
      // The filter bar has its own Clear filters; the empty state offers one too.
      expect(screen.getAllByRole("button", { name: "Clear filters" })).toHaveLength(2);
      await user.click(screen.getAllByRole("button", { name: "Clear filters" })[1] as HTMLElement);
      expect(await screen.findByRole("grid")).toBeTruthy();
      expect(gridRows()).toHaveLength(2);
    });

    it("does not call the store empty while the answer to cleared filters loads", async () => {
      let release: (answer: ReturnType<typeof page>) => void = () => undefined;
      const api = stubApi({
        listMessages: async (query: MessageQuery) => {
          if (query.limit === 1) return page(rows(1, 1), 0);
          return query.filter.states
            ? page([])
            : new Promise((resolve) => {
                release = resolve;
              });
        },
      });
      const { user } = renderRoutes(ROUTES, "/messages?state=invalid", { api });
      await screen.findByText("No messages match");
      await user.click(screen.getAllByRole("button", { name: "Clear filters" })[1] as HTMLElement);
      expect(await screen.findByText("Loading messages…")).toBeTruthy();
      expect(screen.queryByText("No messages yet")).toBeNull();
      release(page(rows(1, 2)));
      expect(await screen.findByRole("grid")).toBeTruthy();
    });
  });

  it("explains a page past the last one and returns to the first", async () => {
    const api = stubApi({
      listMessages: async (query: { offset: number }) =>
        query.offset > 0 ? page([], 40) : page(rows(1, 40), 40),
    });
    const { user, router } = renderRoutes(ROUTES, "/messages?page=5", { api });
    expect(await screen.findByText("This page is past the last message")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Go to the first page" }));
    expect(await screen.findByRole("grid")).toBeTruthy();
    expect(router.state.location.search).toBe("");
  });

  it("shows the error with a retry, and the filter bar keeps working", async () => {
    let fail = true;
    const api = stubApi({
      listMessages: async (query: MessageQuery) => {
        if (fail) throw engineError("internal", "The store is busy.");
        return page(rows(1, query.limit === 1 ? 1 : 2));
      },
    });
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    expect(await screen.findByText("The messages could not be loaded")).toBeTruthy();
    expect(screen.getByText("The store is busy.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /^Needs attention/ }));
    await waitFor(() => expect(lastQuery(api)?.filter.deliverySummaries).toEqual(["attention"]));
    fail = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("grid")).toBeTruthy();
  });

  it("opens a message from a click, or Enter, and the breadcrumb returns to the list as it was", async () => {
    const api = stubApi({
      listMessages: async () => page([summary(7), summary(8)]),
      getMessage: async (id: string) => detail({ summary: summary(Number(id.slice(-6))) }),
    });
    const { user, router } = renderRoutes(ROUTES, "/messages?channel=ch-chem&sort=state", { api });
    await openRow(user, "CTRL-000007");
    expect(await screen.findByRole("heading", { level: 1, name: "CTRL-000007" })).toBeTruthy();
    expect(router.state.location.pathname).toBe("/messages/msg-000007");
    const crumbs = within(screen.getByRole("navigation", { name: "Breadcrumb" }));
    expect(crumbs.getByRole("link", { name: "Messages" }).getAttribute("href")).toBe(
      "/messages?channel=ch-chem&sort=state",
    );
    await user.click(crumbs.getByRole("link", { name: "Messages" }));
    expect(await screen.findByRole("grid")).toBeTruthy();
    expect(router.state.location.search).toBe("?channel=ch-chem&sort=state");
    await openRowWithEnter(user, "CTRL-000008");
    expect(await screen.findByRole("heading", { level: 1, name: "CTRL-000008" })).toBeTruthy();
  });

  describe("resending from a row", () => {
    it("opens the resend dialog from the row's actions menu, with the failed delivery selected", async () => {
      const api = stubApi({
        listMessages: async () =>
          page([
            summary(1, {
              deliverySummary: "attention",
              deliveries: [chip("LIS", { state: "failed", ackLevel: "none" }), chip("Archive")],
            }),
          ]),
      });
      const { user } = renderRoutes(ROUTES, "/messages", { api });
      await screen.findByRole("grid");
      const menu = await openRowActions(user, "CTRL-000001");
      await user.click(within(menu).getByRole("menuitem", { name: "Resend…" }));
      const dialog = await screen.findByRole("dialog", { name: "Resend CTRL-000001" });
      expect(
        (within(dialog).getByRole("checkbox", { name: /^LIS/ }) as HTMLInputElement).checked,
      ).toBe(true);
      expect(
        (within(dialog).getByRole("checkbox", { name: /^Archive/ }) as HTMLInputElement).checked,
      ).toBe(false);
    });

    it("offers no resend for a message that is still arriving or only observed", async () => {
      const api = stubApi({
        listMessages: async () =>
          page([
            summary(1, { state: "receiving", deliveries: [], deliverySummary: "none" }),
            summary(2, { state: "observed", deliveries: [], deliverySummary: "none" }),
            summary(3, { state: "invalid", deliveries: [], deliverySummary: "none" }),
          ]),
      });
      renderRoutes(ROUTES, "/messages", { api });
      await screen.findByRole("grid");
      expect(screen.queryByRole("button", { name: "Actions for CTRL-000001" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Actions for CTRL-000002" })).toBeNull();
      expect(screen.getByRole("button", { name: "Actions for CTRL-000003" })).toBeTruthy();
    });
  });
});
