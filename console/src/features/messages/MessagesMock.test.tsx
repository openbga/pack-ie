import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { MessagePage } from "./MessagePage";
import { MessagesPage } from "./MessagesPage";
import { forgetMessagesSession } from "./messagesSession";

const ROUTES = [
  { path: "/messages", element: <MessagesPage /> },
  { path: "/messages/:id", element: <MessagePage /> },
];

const status = (text: string) =>
  screen.getAllByRole("status").some((element) => element.textContent?.includes(text));

/** The Messages screens against the mock engine, which holds 2,000 synthetic messages. */
describe("Messages against the mock engine", () => {
  let api: MockEngineApi;
  beforeEach(() => {
    forgetMessagesSession();
    mockGridLayout();
    api = new MockEngineApi({ latency: "none", messageCount: 2000 });
  });
  afterEach(() => api.dispose());

  it("pages through the stored messages, one page at a time", async () => {
    const { user } = renderRoutes(ROUTES, "/messages?range=any", { api });
    await screen.findByRole("grid", { name: "Messages" });
    await waitFor(() => expect(status("Rows 1–100 of 2,000")).toBe(true));
    expect(screen.getAllByRole("row").length).toBeLessThanOrEqual(101);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(status("Rows 101–200 of 2,000")).toBe(true));
  });

  it("lists the last 24 hours by default and the counts of its quick filters", async () => {
    renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid", { name: "Messages" });
    const attention = await screen.findByRole("button", { name: /^Needs attention \d/ });
    const count = Number(attention.textContent?.replace(/\D/g, ""));
    expect(count).toBeGreaterThanOrEqual(0);
    const all = await api.listMessages({
      filter: { deliverySummaries: ["attention"] },
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 1,
    });
    expect(all.total).toBeGreaterThanOrEqual(count);
  });

  it("lists the messages that need attention, and opens one with its deliveries", async () => {
    const { user, router } = renderRoutes(ROUTES, "/messages?quick=attention&range=any", { api });
    await screen.findByRole("grid", { name: "Messages" });
    const rows = gridRows("Messages");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(
        within(row).queryAllByText(/^(Failed|Rejected|No acknowledgement|Outcome unknown)$/).length,
      ).toBeGreaterThan(0);
    }
    const first = within(rows[0] as HTMLElement).getAllByRole("gridcell")[0] as HTMLElement;
    await user.click(first);
    await screen.findByRole("heading", { level: 2, name: "Deliveries" });
    expect(router.state.location.pathname).toMatch(/^\/messages\/.+/);
    expect(screen.getByRole("button", { name: "Resend…" })).toBeTruthy();
    // The message opens only on request, as text or as hex.
    expect(screen.queryByRole("region", { name: /Original message as/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    const text = await screen.findByRole("region", { name: "Original message as text" });
    expect(text.textContent?.length).toBeGreaterThan(0);
    await user.click(screen.getByRole("radio", { name: "Hex" }));
    expect(
      (await screen.findByRole("region", { name: "Original message as hex" })).textContent,
    ).toMatch(/^00000000 {2}[0-9a-f]{2} /);
  });

  it("resends a delivery that was rejected and closes another, against the mock's rules", async () => {
    const found = await api.listMessages({
      filter: { deliveryStates: ["rejected"] },
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 1,
    });
    const id = found.rows[0]?.id ?? "";
    const { user } = renderRoutes(ROUTES, `/messages/${id}`, { api });
    await screen.findByRole("heading", { level: 2, name: "Deliveries" });
    await user.click(screen.getByRole("button", { name: "Resend…" }));
    const dialog = await screen.findByRole("dialog", { name: /^Resend / });
    await user.click(within(dialog).getByRole("button", { name: "Resend" }));
    expect(await within(dialog).findByText(/1 of \d+ destinations? accepted\./)).toBeTruthy();
    expect(within(dialog).getByText(/Queued again/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("closes a delivery with a reason, and the message stops needing attention", async () => {
    const found = await api.listMessages({
      filter: { deliveryStates: ["failed", "rejected"], deliverySummaries: ["attention"] },
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 1000,
    });
    const single = (await Promise.all(found.rows.map((row) => api.getMessage(row.id)))).find(
      (d) =>
        d.deliveries.filter((x) =>
          ["failed", "rejected", "timed_out", "outcome_unknown"].includes(x.state),
        ).length === 1 && d.deliveries.some((x) => x.state === "rejected" || x.state === "failed"),
    );
    const target = single?.deliveries.find((d) => d.state === "rejected" || d.state === "failed");
    const { user } = renderRoutes(ROUTES, `/messages/${single?.summary.id}`, { api });
    await screen.findByRole("heading", { level: 2, name: "Deliveries" });
    await user.click(
      screen.getByRole("button", { name: `Close delivery to ${target?.destinationLabel}` }),
    );
    await user.type(await screen.findByRole("textbox", { name: /Reason/ }), "Entered by hand");
    await user.click(screen.getByRole("button", { name: "Close delivery" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(screen.queryByText("Needs attention")).toBeNull());
    expect(await screen.findByText("Entered by hand")).toBeTruthy();
  });

  it("finds a message by the identifier of one that exists", async () => {
    const first = await api.listMessages({
      filter: { states: ["routed"] },
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 1,
    });
    const target = first.rows[0];
    if (!target) throw new Error("the mock has no messages");
    const identifier = target.controlId ?? target.sampleId ?? "";
    const { user } = renderRoutes(ROUTES, "/messages?range=any", { api });
    await screen.findByRole("grid", { name: "Messages" });
    await user.type(screen.getByRole("searchbox", { name: "Identifier" }), `${identifier}{Enter}`);
    await waitFor(() =>
      expect(within(screen.getByRole("grid")).getAllByText(identifier).length).toBeGreaterThan(0),
    );
  });

  it("finds one message by the start of its ID under More filters", async () => {
    const first = await api.listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 7,
      limit: 1,
    });
    const target = first.rows[0];
    if (!target) throw new Error("the mock has no messages");
    // The mock numbers its messages `msg-7`; its stored (UUID) form ends in the number in hex.
    const stored = `00000000-0000-4000-8000-${Number(target.id.slice(4)).toString(16).padStart(12, "0")}`;
    const { user } = renderRoutes(ROUTES, "/messages?range=any", { api });
    await screen.findByRole("grid", { name: "Messages" });
    await user.click(screen.getByRole("button", { name: "More filters" }));
    await user.type(screen.getByRole("searchbox", { name: "Message ID" }), `${stored}{Enter}`);
    await waitFor(() => expect(status("Rows 1–1 of 1")).toBe(true));
    expect(within(screen.getByRole("grid")).getAllByRole("row")).toHaveLength(2);
  });
});
