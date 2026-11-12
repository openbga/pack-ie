import { screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { DeliveryChip, EngineError, MessagePage, MessageSummary } from "@/api/types";
import { renderRoutes } from "@/lib/test/router";
import {
  ago,
  channelDetail,
  destination,
  endpoint,
  engineError,
  messageChannel,
  snapshot,
  stubApi,
} from "../shared-operate/testFixtures";
import { channelRoutes } from "./testFixtures";

const message = (n: number, overrides: Partial<MessageSummary> = {}): MessageSummary => ({
  id: `msg-${n}`,
  channelId: "ch-hl7",
  channelLabel: "Results to LIS",
  seq: n,
  direction: "inbound",
  state: "routed",
  origin: "received",
  receivedAt: ago(n * 60),
  protocol: "hl7_mllp",
  messageType: "ORU^R01",
  controlId: `CTRL-${n}`,
  sampleId: null,
  patientId: null,
  sizeBytes: 400,
  sourcePeer: null,
  configVersion: 7,
  resentFrom: null,
  deliverySummary: "complete",
  deliveries: [
    {
      deliveryId: `dlv-${n}`,
      destinationId: "dst-1",
      destinationLabel: "LIS results",
      state: "acknowledged",
      ackLevel: "accepted",
    } satisfies DeliveryChip,
  ],
  ...overrides,
});

const page = (rows: MessageSummary[]): MessagePage => ({
  rows,
  total: rows.length,
  offset: 0,
  limit: 10,
});

function render(
  live = messageChannel(),
  destinations = [destination()],
  api: Record<string, unknown> = {},
) {
  return renderRoutes(channelRoutes, `/channels/${live.id}`, {
    api: stubApi({
      getChannel: async () => channelDetail(live, destinations),
      getSnapshot: async () =>
        snapshot({
          channels: [live],
          endpoints: [endpoint({ id: "ep-lis", consecutiveFailureCount: 4 })],
        }),
      queryLogs: async () => ({ events: [], olderCursor: null }),
      listMessages: async () => page([]),
      ...api,
    }),
  });
}

describe("ChannelPage, a degraded message channel", () => {
  const failing = destination({
    destinationId: "dst-backup",
    label: "Backup LIS",
    endpoint: { endpointId: "ep-lis", label: "Backup LIS link", state: "retrying" },
    queued: 30,
    nextAttemptAt: new Date(Date.now() + 21_000).toISOString(),
    lastError: {
      at: ago(30),
      summary: "connection refused (10.0.4.21:2575)",
      nextAction: "Check the standby LIS.",
    },
  });
  const degraded = () =>
    messageChannel({
      state: "degraded",
      lastError: failing.lastError,
      nextRetryAt: null,
    });

  it("gives the error the failing destination's streak and its next attempt, not the source's", async () => {
    render(degraded(), [failing]);
    await screen.findByText("Failure streak");
    const note = screen.getAllByText("Last error")[0]?.closest("[role]") as HTMLElement;
    const text = note.textContent ?? "";
    expect(text).toContain("Failure streak4");
    expect(text).toContain("Destination Backup LIS: Backup LIS link");
    expect(text).toMatch(/Delivery retryin \d+ s/);
    expect(text).toContain("Source sessionReceiving (Chemistry source)");
    expect(text).not.toContain("Not retrying");
  });

  it("says no attempt is scheduled, and why, when the destination is disabled", async () => {
    render(degraded(), [{ ...failing, enabled: false, nextAttemptAt: null }]);
    expect(
      (await screen.findAllByText(/No attempt is scheduled: the destination is disabled/)).length,
    ).toBe(1);
  });

  it("does not say Not retrying for a degraded channel whose destination retries", async () => {
    const live = messageChannel({ state: "degraded", lastError: null });
    render(live, [failing]);
    expect(
      await screen.findByText(/Source session is healthy\. Delivery to Backup LIS retries/),
    ).toBeTruthy();
    expect(screen.queryByText("Not retrying")).toBeNull();
  });

  it("shows the mapping failures of a destination, with the way out", async () => {
    const mapped = destination({
      label: "Archive",
      unresolved: { rejected: 0, failed: 5, timed_out: 0, outcome_unknown: 0 },
      transformErrors: 3,
    });
    render(messageChannel({ queue: { depth: 5, needsAttention: 5, oldestQueuedAt: ago(30) } }), [
      mapped,
    ]);
    const card = await screen.findByRole("region", { name: "Destination Archive" });
    expect(card.textContent).toContain("5 failed (3 in the mapping)");
    expect(card.textContent).toContain(
      "3 of them: the mapping failed — fix the mapping, then resend.",
    );
    expect(within(card).getByRole("link", { name: "Edit the mapping" }).getAttribute("href")).toBe(
      "/channels/ch-hl7/edit",
    );
    expect(
      within(card).getByRole("link", { name: "View failed deliveries" }).getAttribute("href"),
    ).toBe("/messages?channel=ch-hl7&delivery=failed&range=any");
    expect(screen.getByText(/3 failed in the mapping, never sent/)).toBeTruthy();
  });

  it("says nothing of mappings when none failed", async () => {
    render(messageChannel(), [destination({ transformErrors: 0 })]);
    await screen.findByRole("region", { name: "Destination LIS results" });
    expect(screen.queryByText(/the mapping failed/)).toBeNull();
  });
});

describe("ChannelPage, recent messages", () => {
  it("lists the newest ten with the time, identifier, state and deliveries, each linking to its record", async () => {
    const listMessages = vi.fn(async () =>
      page([message(1), message(2, { controlId: null, sampleId: "SMP-9" })]),
    );
    render(messageChannel(), [destination()], { listMessages });
    const table = await screen.findByRole("table", { name: "Recent messages of this channel" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(rows[1]?.textContent).toContain("CTRL-1");
    expect(rows[1]?.textContent).toContain("Routed");
    expect(rows[1]?.textContent).toContain("LIS results");
    expect(rows[2]?.textContent).toContain("SMP-9");
    const link = within(rows[1] as HTMLElement).getByRole("link");
    expect(link.getAttribute("href")).toBe("/messages/msg-1");
    expect(link.querySelector("time")?.getAttribute("title")).not.toBeNull();
    expect(screen.getByRole("link", { name: "View all" }).getAttribute("href")).toBe(
      "/messages?channel=ch-hl7&range=any",
    );
    expect(listMessages).toHaveBeenCalledWith({
      filter: { channelIds: ["ch-hl7"] },
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 10,
    });
  });

  it("says calmly that the channel has not received a message", async () => {
    render();
    expect(await screen.findByText("This channel has not received a message yet.")).toBeTruthy();
  });

  it("says the store is not available when the engine does not serve messages", async () => {
    const notImplemented: EngineError = engineError("not_implemented");
    render(messageChannel(), [destination()], {
      listMessages: async () => {
        throw notImplemented;
      },
    });
    expect(await screen.findByText("The message store is not available yet.")).toBeTruthy();
  });

  it("shows a read error with a retry", async () => {
    render(messageChannel(), [destination()], {
      listMessages: async () => {
        throw engineError("unavailable", "The engine did not answer.");
      },
    });
    expect(await screen.findByText("The messages could not be loaded")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("is not on a bridge's page", async () => {
    renderRoutes(channelRoutes, "/channels/ch-alpha", {
      api: stubApi({
        getChannel: async () => channelDetail(),
        getSnapshot: async () => snapshot(),
        queryLogs: async () => ({ events: [], olderCursor: null }),
      }),
    });
    await screen.findByRole("heading", { level: 1, name: "Alpha bridge" });
    expect(screen.queryByRole("heading", { name: "Recent messages" })).toBeNull();
  });
});
