import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { status } from "@/lib/test/liveStatus";
import { renderRoutes } from "@/lib/test/router";
import { chooseOption } from "@/lib/test/select";
import { chip, page, rows, snapshot, stubApi, summary } from "./fixtures";
import { lastQuery, listQueries, ROUTES } from "./messagesListSetup";
import { forgetMessagesSession } from "./messagesSession";

describe("MessagesPage", () => {
  beforeEach(() => {
    mockGridLayout();
    forgetMessagesSession();
  });

  it("shows one page of messages with the default columns, and has no violations", async () => {
    const api = stubApi({
      listMessages: async () =>
        page(
          [
            summary(1, {
              deliveries: [chip("LIS"), chip("Archive", { state: "failed", ackLevel: "none" })],
              deliverySummary: "attention",
            }),
            ...rows(2, 2),
          ],
          412_455,
        ),
    });
    const { container } = renderRoutes(ROUTES, "/messages", { api });
    const grid = await screen.findByRole("grid", { name: "Messages" });
    expect(
      within(grid)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual([
      "Received",
      "Channel",
      "Direction",
      "Message state",
      "Delivery summary",
      "Message type",
      "Identifier",
      "Patient ID",
      "Size",
      "Actions",
    ]);
    const first = within(gridRows()[0] as HTMLElement);
    expect(first.getByText("CTRL-000001")).toBeTruthy();
    expect(first.getByText("PAT-TEST-000001")).toBeTruthy();
    expect(first.getByText("ORU^R01")).toBeTruthy();
    expect(first.getByText("Routed")).toBeTruthy();
    expect(first.getByText("Inbound")).toBeTruthy();
    expect(first.getByText("1.5 KB")).toBeTruthy();
    expect(first.getByText("Accepted")).toBeTruthy();
    expect(first.getByText("Failed")).toBeTruthy();
    expect(first.getByText("Archive")).toBeTruthy();
    expect(grid.getAttribute("aria-rowcount")).toBe("412456");
    await waitFor(() => expect(status("Rows 1–3 of 412,455")).toBe(true));
    await expectNoA11yViolations(container, { wholePage: false });
  });

  it("shows the sample identifier when there is no control id, and says what is not provided", async () => {
    const api = stubApi({
      listMessages: async () =>
        page([
          summary(1, { controlId: null, sampleId: "SMP-9" }),
          summary(2, {
            controlId: null,
            sampleId: null,
            patientId: null,
            messageType: null,
            state: "invalid",
            deliverySummary: "none",
            deliveries: [],
          }),
        ]),
    });
    renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    const [one, two] = gridRows() as [HTMLElement, HTMLElement];
    expect(within(one).getByText("SMP-9")).toBeTruthy();
    expect(within(two).getAllByText("Not provided")).toHaveLength(3);
    expect(within(two).getByText("Invalid")).toBeTruthy();
    expect(within(two).getByText("No deliveries")).toBeTruthy();
  });

  it("labels sample messages as such", async () => {
    const api = stubApi({
      capabilities: { messages: "demo", secretsWritable: false, alertHistory: false },
    });
    renderRoutes(ROUTES, "/messages", { api });
    expect(await screen.findByText("Sample messages")).toBeTruthy();
  });

  describe("when the store cannot be read", () => {
    it("says so, with the reason, when the engine does not serve it yet", async () => {
      const api = stubApi({
        listMessages: async () => {
          throw engineError("not_implemented", "No store.");
        },
      });
      renderRoutes(ROUTES, "/messages", { api });
      expect(await screen.findByText("The message store is not available")).toBeTruthy();
      expect(screen.getByText(/does not serve its stored messages yet/)).toBeTruthy();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.queryByRole("grid")).toBeNull();
      expect(screen.queryByText(/could not be loaded/)).toBeNull();
    });

    it.each([
      ["unavailable", /reports that its message store is unavailable/],
      ["not_configured", /runs without a message store/],
    ] as const)("says so when the snapshot reports the store %s", async (state, reason) => {
      const api = stubApi({
        getSnapshot: async () =>
          snapshot({
            state,
            messageCount: null,
            databaseBytes: null,
            diskFreeBytes: null,
            diskTotalBytes: null,
          }),
        listMessages: async () => page([]),
      });
      renderRoutes(ROUTES, "/messages", { api });
      expect(await screen.findByText("The message store is not available")).toBeTruthy();
      expect(screen.getByText(reason)).toBeTruthy();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.queryByRole("grid")).toBeNull();
      expect(screen.queryByText("No messages yet")).toBeNull();
    });
  });

  it("asks the engine for one page of the last 24 hours, newest first, and never for more", async () => {
    const api = stubApi();
    const before = Date.now();
    renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    const query = lastQuery(api);
    expect(query).toMatchObject({
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 100,
    });
    const since = Date.parse(query?.filter.since ?? "");
    expect(since).toBeGreaterThanOrEqual(before - 86_400_000);
    expect(since).toBeLessThanOrEqual(Date.now() - 86_400_000);
    expect(query?.filter).toEqual({ since: query?.filter.since });
  });

  it("passes the filters in the URL to the engine", async () => {
    const api = stubApi();
    renderRoutes(
      ROUTES,
      "/messages?channel=ch-chem&channel=ch-hema&direction=a_to_b&state=invalid&delivery=failed&range=any&sort=state&page=2",
      { api },
    );
    await screen.findByRole("grid");
    expect(lastQuery(api)).toEqual({
      filter: {
        channelIds: ["ch-chem", "ch-hema"],
        directions: ["a_to_b"],
        states: ["invalid"],
        deliveryStates: ["failed"],
      },
      sort: { field: "state", descending: false },
      offset: 100,
      limit: 100,
    });
    expect(screen.getByRole("button", { name: /Channels/ }).textContent).toContain("2 selected");
    expect(screen.getByRole("button", { name: /Time range/ }).textContent).toContain("Any time");
  });

  it("applies a quick filter from the URL, and shows it pressed", async () => {
    const api = stubApi();
    renderRoutes(ROUTES, "/messages?quick=invalid_aborted", { api });
    await screen.findByRole("grid");
    expect(lastQuery(api)?.filter).toMatchObject({ states: ["invalid", "aborted"] });
    expect(
      screen.getByRole("button", { name: /^Invalid or aborted/ }).getAttribute("aria-pressed"),
    ).toBe("true");
  });

  it("resolves a quick range when it reads, not when it was chosen", async () => {
    const api = stubApi();
    const before = Date.now();
    renderRoutes(ROUTES, "/messages?from=-1h", { api });
    await screen.findByRole("grid");
    const since = Date.parse(lastQuery(api)?.filter.since ?? "");
    expect(since).toBeGreaterThanOrEqual(before - 3_600_000);
    expect(since).toBeLessThanOrEqual(Date.now() - 3_600_000);
  });

  it("keeps the identifier and text out of the address, and sends them to the engine", async () => {
    const api = stubApi();
    const { user, router } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.type(screen.getByRole("searchbox", { name: "Identifier" }), "PAT-TEST-7{Enter}");
    await user.type(screen.getByRole("searchbox", { name: "Message text" }), "glucose{Enter}");
    await waitFor(() =>
      expect(lastQuery(api)?.filter).toMatchObject({
        identifier: { value: "PAT-TEST-7", prefix: true },
        text: "glucose",
      }),
    );
    expect(router.state.location.search).toBe("");
    expect(router.state.location.search).not.toMatch(/PAT-TEST|glucose/);
    expect(screen.getByText("Text search covers the selected time range only.")).toBeTruthy();
  });

  it("does not search for a text shorter than 3 characters", async () => {
    const api = stubApi();
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    const calls = listQueries(api).length;
    await user.type(screen.getByRole("searchbox", { name: "Message text" }), "ab{Enter}");
    expect(await screen.findByText("Type at least 3 characters to search text.")).toBeTruthy();
    expect(lastQuery(api)?.filter.text).toBeUndefined();
    expect(listQueries(api).every((query) => query.filter.text === undefined)).toBe(true);
    expect(listQueries(api).length).toBeGreaterThanOrEqual(calls);
  });

  it("pages with Next by the next offset and keeps the page in the URL", async () => {
    const api = stubApi({
      listMessages: async (query: { offset: number; limit: number }) =>
        query.limit === 1 ? page(rows(1, 1), 0) : page(rows(query.offset + 1, 100), 250),
    });
    const { user, router } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(lastQuery(api)?.offset).toBe(100));
    expect(router.state.location.search).toBe("?page=2");
    await waitFor(() => expect(status("Rows 101–200 of 250")).toBe(true));
    expect(screen.getByRole("gridcell", { name: "CTRL-000101" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Previous" }));
    await waitFor(() => expect(router.state.location.search).toBe(""));
    expect(await screen.findByRole("gridcell", { name: "CTRL-000001" })).toBeTruthy();
  });

  it("changes the page size and starts again from the first page", async () => {
    const api = stubApi({
      listMessages: async (query: { offset: number; limit: number }) =>
        page(rows(query.offset + 1, query.limit), 1_000),
    });
    const { user, router } = renderRoutes(ROUTES, "/messages?page=3", { api });
    await screen.findByRole("grid");
    await chooseOption(user, /Rows per page/, "50");
    await waitFor(() => expect(lastQuery(api)).toMatchObject({ offset: 0, limit: 50 }));
    expect(router.state.location.search).toBe("");
  });
});
