import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditQuery } from "@/api/types";
import { IdleProvider } from "@/lib/idle";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { columnTexts, openRow, openRowWithEnter } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { status } from "@/lib/test/liveStatus";
import { renderRoutes } from "@/lib/test/router";
import { chooseOption } from "@/lib/test/select";
import { AuditPage } from "./AuditPage";
import { auditEvent, auditPage, hashOf, newestFirst, type StubApi, stubApi } from "./fixtures";

const ROUTES = [{ path: "/audit", element: <AuditPage /> }];

/** The Action cell text of each row shown, top to bottom. */
const actions = () => columnTexts(2);

async function open(api: StubApi, path = "/audit") {
  const rendered = renderRoutes(ROUTES, path, { api });
  await screen.findByRole("grid", { name: "Audit events" });
  return rendered;
}

const lastQuery = (api: StubApi): AuditQuery => {
  const call = api.getAuditEvents.mock.calls.at(-1);
  if (!call) throw new Error("no query");
  return call[0];
};

describe("AuditPage", () => {
  beforeEach(mockGridLayout);

  it("lists the newest events first, in the audit columns, and has no violations", async () => {
    const api = stubApi({
      getAuditEvents: async () =>
        auditPage([
          auditEvent(3, {
            action: "config_apply",
            objectType: null,
            objectId: null,
            outcome: "rejected",
            origin: "cli",
          }),
          auditEvent(2, { action: "message_resend", objectType: "message", objectId: "msg-9" }),
          auditEvent(1, { outcome: "failed" }),
        ]),
    });
    const { container } = await open(api);
    expect(
      within(screen.getByRole("grid"))
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["Time", "Actor", "Action", "Object", "Outcome"]);
    expect(actions()).toEqual(["Configuration applied", "Message resent", "Channel paused"]);
    const first = within(screen.getAllByRole("row")[1] as HTMLElement);
    expect(first.getByText("LAB\\jdoe")).toBeTruthy();
    expect(first.getByText("Rejected")).toBeTruthy();
    const second = within(screen.getAllByRole("row")[2] as HTMLElement);
    expect(second.getByRole("link", { name: "msg-9" }).getAttribute("href")).toBe(
      "/messages/msg-9",
    );
    expect(within(screen.getAllByRole("row")[3] as HTMLElement).getByText("Failed")).toBeTruthy();
    expect(status("3 events loaded · no older events")).toBe(true);
    await expectNoA11yViolations(container);
  });

  it("offers Origin as an optional column, and a way back to the default columns", async () => {
    const api = stubApi({
      getAuditEvents: async () => auditPage([auditEvent(1, { origin: "cli" })]),
    });
    const { user } = await open(api);
    const headers = () =>
      within(screen.getByRole("grid", { hidden: true }))
        .getAllByRole("columnheader", { hidden: true })
        .map((header) => header.textContent);
    expect(screen.queryByText("Command line")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Columns: Default" }));
    const chooser = await screen.findByRole("dialog", { name: "Columns" });
    expect(
      within(chooser)
        .getAllByRole("checkbox")
        .map((box) => box.closest("label")?.textContent),
    ).toEqual(["Actor", "Origin", "Object"]);
    await user.click(within(chooser).getByRole("checkbox", { name: "Origin" }));
    expect(headers()).toEqual(["Time", "Actor", "Origin", "Action", "Object", "Outcome"]);
    expect(screen.getByText("Command line")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Columns: Custom", hidden: true })).toBeTruthy();
    await user.click(within(chooser).getByRole("button", { name: "Default columns" }));
    expect(headers()).toEqual(["Time", "Actor", "Action", "Object", "Outcome"]);
  });

  it("asks for the newest 100 events with no filters", async () => {
    const api = stubApi();
    await open(api);
    expect(api.getAuditEvents).toHaveBeenCalledWith({ filter: {}, beforeSeq: null, limit: 100 });
  });

  it("sends the filters of the address, and the exact text as typed", async () => {
    const api = stubApi();
    await open(
      api,
      "/audit?actor=LAB%5Cjdoe&action=channel_pause&type=channel&id=ch-1&outcome=failed",
    );
    expect(lastQuery(api).filter).toEqual({
      actor: "LAB\\jdoe",
      action: "channel_pause",
      objectType: "channel",
      objectId: "ch-1",
      outcome: "failed",
    });
  });

  it("counts a time preset back from now and sends a custom range as given", async () => {
    const api = stubApi();
    const before = Date.now();
    await open(api, "/audit?range=24h");
    const since = lastQuery(api).filter.since;
    expect(since).toBeDefined();
    const back = new Date(since ?? "").getTime() - (before - 86_400_000);
    expect(back).toBeGreaterThanOrEqual(0);
    expect(back).toBeLessThan(5_000);
    expect(lastQuery(api).filter.until).toBeUndefined();

    const custom = stubApi();
    await open(
      custom,
      "/audit?range=custom&from=2026-11-01T00:00:00.000Z&to=2026-11-02T00:00:00.000Z",
    );
    expect(lastQuery(custom).filter).toEqual({
      since: "2026-11-01T00:00:00.000Z",
      until: "2026-11-02T00:00:00.000Z",
    });
  });

  it("changes the request and the address when a filter is chosen, and clears them", async () => {
    const api = stubApi();
    const { user, router } = await open(api);
    expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
    await chooseOption(user, /Action/, "Channel resumed");
    await waitFor(() => expect(lastQuery(api).filter).toEqual({ action: "channel_resume" }));
    expect(router.state.location.search).toBe("?action=channel_resume");
    await chooseOption(user, /Outcome/, "Rejected");
    await waitFor(() =>
      expect(lastQuery(api).filter).toEqual({ action: "channel_resume", outcome: "rejected" }),
    );
    await chooseOption(user, /Time range/, "Last 7 days");
    await waitFor(() => expect(lastQuery(api).filter.since).toBeDefined());
    expect(router.state.location.search).toBe("?range=7d&action=channel_resume&outcome=rejected");
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(lastQuery(api).filter).toEqual({}));
    expect(router.state.location.search).toBe("");
  });

  it("reports typed actor and object ID once typing pauses, trimmed", async () => {
    const api = stubApi();
    const { user, router } = await open(api);
    await user.type(screen.getByRole("searchbox", { name: "Actor" }), "  LAB\\mlopez ");
    await user.type(screen.getByRole("searchbox", { name: "Object ID" }), "ch-2{Enter}");
    await waitFor(() =>
      expect(lastQuery(api).filter).toEqual({ actor: "LAB\\mlopez", objectId: "ch-2" }),
    );
    expect(new URLSearchParams(router.state.location.search).get("actor")).toBe("LAB\\mlopez");
  });

  it("loads older events with the cursor and the same filter, then says there are no more", async () => {
    const api = stubApi({
      getAuditEvents: async (query: AuditQuery) =>
        query.beforeSeq === null ? auditPage(newestFirst(5, 3), 3) : auditPage(newestFirst(2, 1)),
    });
    const { user } = await open(api, "/audit?action=channel_pause");
    expect(status("3 events loaded · older events available")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Load older events" }));
    await waitFor(() => expect(status("5 events loaded · no older events")).toBe(true));
    expect(lastQuery(api)).toEqual({
      filter: { action: "channel_pause" },
      beforeSeq: 3,
      limit: 100,
    });
    expect(screen.queryByRole("button", { name: "Load older events" })).toBeNull();
    expect(screen.getByText("No older events.")).toBeTruthy();
  });

  describe("detail", () => {
    const WITH_DETAILS = auditEvent(2, {
      action: "config_apply",
      objectType: null,
      objectId: null,
      details: {
        base_version: 41,
        note: "Raise baud rate after cable swap for patient SYN-123456",
        destinations: ["dest-a", "dest-b"],
      },
    });
    const api = () =>
      stubApi({
        getAuditEvents: async () => auditPage([auditEvent(3), WITH_DETAILS, auditEvent(1)]),
      });

    it("opens the event with its facts and details as field and value rows, and puts it in the address", async () => {
      const { user, router } = await open(api());
      await openRow(user, "Configuration applied");
      const panel = screen.getByRole("region", { name: "Event 2" });
      expect(router.state.location.search).toBe("?seq=2");
      expect(within(panel).getByText("S-1-5-21-1001")).toBeTruthy();
      expect(within(panel).getByText("config_apply")).toBeTruthy();
      expect(within(panel).getByText("Configuration applied", { exact: false })).toBeTruthy();
      expect(within(panel).getByText(hashOf(2))).toBeTruthy();
      expect(within(panel).getByText(hashOf(1))).toBeTruthy();
      expect(within(panel).getByText("None")).toBeTruthy();
      expect(within(panel).getByText("Base version")).toBeTruthy();
      expect(within(panel).getByText("base_version")).toBeTruthy();
      expect(within(panel).getByText("41")).toBeTruthy();
      expect(
        within(panel).getByText("Raise baud rate after cable swap for patient SYN-123456"),
      ).toBeTruthy();
      expect(within(panel).getByText('["dest-a","dest-b"]')).toBeTruthy();
      expect(within(panel).getByText("2026-11-06 12:02:00.000 UTC", { exact: false })).toBeTruthy();
    });

    it("opens with Enter, steps to newer and older events, and closes with Escape", async () => {
      const { user, router } = await open(api());
      await openRowWithEnter(user, "Configuration applied");
      expect(screen.getByRole("region", { name: "Event 2" })).toBeTruthy();
      await user.click(screen.getByRole("button", { name: "Next event (older)" }));
      expect(screen.getByRole("region", { name: "Event 1" })).toBeTruthy();
      expect(
        screen.getByRole("button", { name: "Next event (older)" }).hasAttribute("disabled"),
      ).toBe(true);
      await user.click(screen.getByRole("button", { name: "Previous event (newer)" }));
      await user.click(screen.getByRole("button", { name: "Previous event (newer)" }));
      expect(screen.getByRole("region", { name: "Event 3" })).toBeTruthy();
      expect(router.state.location.search).toBe("?seq=3");
      act(() => screen.getByRole("button", { name: "Next event (older)" }).focus());
      await user.keyboard("{Escape}");
      expect(screen.queryByRole("region", { name: /^Event/ })).toBeNull();
      expect(router.state.location.search).toBe("");
    });

    it("shows an event of the address that is not among those loaded, read on its own", async () => {
      const stub = stubApi({
        getAuditEvents: async (query: AuditQuery) =>
          query.limit === 1 ? auditPage([auditEvent(40)]) : auditPage(newestFirst(120, 61)),
      });
      await open(stub, "/audit?seq=40");
      await screen.findByText("S-1-5-21-1001");
      expect(screen.getByRole("region", { name: "Event 40" })).toBeTruthy();
      expect(stub.getAuditEvents).toHaveBeenCalledWith({ filter: {}, beforeSeq: 41, limit: 1 });
    });

    it("says so when the event of the address does not exist", async () => {
      const stub = stubApi({
        getAuditEvents: async (query: AuditQuery) =>
          query.limit === 1 ? auditPage([]) : auditPage(newestFirst(3, 1)),
      });
      await open(stub, "/audit?seq=99");
      expect(await screen.findByText(/There is no event 99/)).toBeTruthy();
      expect(screen.getByRole("region", { name: "Event 99" })).toBeTruthy();
    });
  });

  describe("idle protection", () => {
    beforeEach(() => {
      localStorage.clear();
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });
    afterEach(() => vi.useRealTimers());

    it("closes the event detail when the console goes idle", async () => {
      const stub = stubApi({
        getAuditEvents: async () =>
          auditPage([auditEvent(1, { action: "config_apply", details: { note: "SYN-123456" } })]),
      });
      const { router } = renderRoutes(
        [
          {
            path: "/audit",
            element: (
              <IdleProvider>
                <AuditPage />
              </IdleProvider>
            ),
          },
        ],
        "/audit?seq=1",
        { api: stub },
      );
      await screen.findByText("SYN-123456");
      act(() => {
        vi.advanceTimersByTime(14 * 60_000);
      });
      expect(screen.getByText("SYN-123456")).toBeTruthy();
      act(() => {
        vi.advanceTimersByTime(61_000);
      });
      // The details are gone from the page and from the address; the list stays.
      expect(screen.queryByText("SYN-123456")).toBeNull();
      expect(screen.queryByRole("region", { name: "Event 1" })).toBeNull();
      expect(router.state.location.search).toBe("");
      expect(screen.getByRole("grid", { name: "Audit events" })).toBeTruthy();
    });
  });

  describe("states", () => {
    it("says the audit log has no events yet", async () => {
      const stub = stubApi({ getAuditEvents: async () => auditPage([]) });
      renderRoutes(ROUTES, "/audit", { api: stub });
      expect(await screen.findByText("No audit events yet")).toBeTruthy();
      expect(screen.queryByRole("grid")).toBeNull();
    });

    it("says no event matches the filters, that actor and object ID match exactly, and offers to clear them", async () => {
      const stub = stubApi({ getAuditEvents: async () => auditPage([]) });
      const { user, router } = renderRoutes(ROUTES, "/audit?actor=jdoe", { api: stub });
      expect(await screen.findByText("No audit events match the filters")).toBeTruthy();
      expect(screen.getByText(/Actor and object ID must match exactly/)).toBeTruthy();
      await user.click(screen.getAllByRole("button", { name: "Clear filters" })[0] as HTMLElement);
      await waitFor(() => expect(router.state.location.search).toBe(""));
    });

    it("shows a loading state while the engine answers", async () => {
      const stub = stubApi({ getAuditEvents: () => new Promise(() => undefined) });
      renderRoutes(ROUTES, "/audit", { api: stub });
      expect(await screen.findByText("Loading audit events…")).toBeTruthy();
      expect(screen.getByRole("heading", { level: 1, name: "Audit" })).toBeTruthy();
    });

    it("explains an unreadable audit log, that audited actions are refused, and retries", async () => {
      let fail = true;
      const stub = stubApi({
        getAuditEvents: async () => {
          if (fail) throw engineError("unavailable", "The audit log cannot be read.");
          return auditPage(newestFirst(2, 1));
        },
      });
      const { user } = renderRoutes(ROUTES, "/audit", { api: stub });
      const alert = await screen.findByRole("alert");
      expect(within(alert).getByText(/Audited actions are refused/)).toBeTruthy();
      expect(within(alert).getByText("The audit log cannot be read.")).toBeTruthy();
      fail = false;
      await user.click(within(alert).getByRole("button", { name: "Try again" }));
      await screen.findByRole("grid", { name: "Audit events" });
    });

    it("shows another failure with its reference and a retry", async () => {
      const stub = stubApi({
        getAuditEvents: async () => {
          throw engineError("engine_unavailable", "The engine is not running.");
        },
      });
      renderRoutes(ROUTES, "/audit", { api: stub });
      const alert = await screen.findByRole("alert");
      expect(within(alert).getByText("The audit events could not be loaded")).toBeTruthy();
      expect(within(alert).getByText("corr-test-0001")).toBeTruthy();
    });
  });
});
