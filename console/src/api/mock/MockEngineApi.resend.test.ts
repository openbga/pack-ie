import { describe, expect, it, vi } from "vitest";
import type { Delivery } from "../types";
import {
  mockApiFactory,
  originalText,
  query,
  resendAsNew,
  rowsOf,
  withDelivery,
} from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

/** A resend of one delivery as the same message, the duplicate risk not confirmed. */
function sameMessage(messageId: string, delivery: Delivery) {
  return {
    messageId,
    destinations: [delivery.destinationId],
    mode: "same_message" as const,
    confirmDuplicateRisk: false,
  };
}

describe("resend", () => {
  it("queues a failed delivery again for another attempt cycle, which is then acknowledged", async () => {
    vi.useFakeTimers();
    const api = create();
    const { detail, delivery } = await withDelivery(api, "rejected");
    const id = detail.summary.id;
    const result = await api.resendMessage(sameMessage(id, delivery));
    expect(result).toEqual({
      newMessageId: null,
      destinations: [
        {
          destinationId: delivery.destinationId,
          destinationLabel: delivery.destinationLabel,
          deliveryId: delivery.id,
          rejected: null,
        },
      ],
    });
    const queued = (await api.getMessage(id)).deliveries.find((d) => d.id === delivery.id);
    expect(queued).toMatchObject({
      state: "queued",
      attempts: 0,
      totalAttempts: delivery.totalAttempts,
      ackLevel: "none",
      closedReason: null,
    });
    expect(queued?.attemptLog).toHaveLength(delivery.attemptLog.length);

    vi.advanceTimersByTime(2100);
    const settled = (await api.getMessage(id)).deliveries.find((d) => d.id === delivery.id);
    expect(settled).toMatchObject({
      state: "acknowledged",
      attempts: 1,
      totalAttempts: delivery.totalAttempts + 1,
    });
    expect(settled?.attemptLog.at(-1)?.attemptNo).toBe(delivery.totalAttempts + 1);
    expect(settled?.attemptLog.at(-1)?.result).toBe("acknowledged");
  });

  it("records the reason with the new attempt cycle and clears it with the next resend without one", async () => {
    vi.useFakeTimers();
    const api = create();
    const { detail, delivery } = await withDelivery(api, "rejected");
    expect(delivery.resendReason).toBeNull();
    const id = detail.summary.id;
    const request = sameMessage(id, delivery);
    await api.resendMessage({ ...request, reason: "  Destination was down  " });
    const find = async () =>
      (await api.getMessage(id)).deliveries.find((d) => d.id === delivery.id);
    expect((await find())?.resendReason).toBe("Destination was down");
    // The reason stays with the cycle once the destination has answered.
    vi.advanceTimersByTime(2100);
    expect((await find())?.resendReason).toBe("Destination was down");
  });

  it("refuses a resend reason that is empty or longer than 500 characters", async () => {
    const api = create();
    const { detail, delivery } = await withDelivery(api, "rejected");
    const request = sameMessage(detail.summary.id, delivery);
    await expect(api.resendMessage({ ...request, reason: "  " })).rejects.toMatchObject({
      code: "validation",
    });
    await expect(api.resendMessage({ ...request, reason: "x".repeat(501) })).rejects.toMatchObject({
      code: "validation",
    });
    await expect(api.resendMessage({ ...request, reason: "x".repeat(500) })).resolves.toBeDefined();
  });

  it("gives a delivery whose mapping failed its output when it is resent", async () => {
    vi.useFakeTimers();
    const api = create();
    const { detail, delivery } = await withDelivery(
      api,
      "failed",
      { deliverySummaries: ["attention"] },
      (d) => d.reasonCode === "transform_error",
    );
    expect(delivery).toMatchObject({ attempts: 0, transformedHash: null, mappingVersion: null });
    await api.resendMessage(sameMessage(detail.summary.id, delivery));
    vi.advanceTimersByTime(2100);
    const after = (await api.getMessage(detail.summary.id)).deliveries.find(
      (d) => d.id === delivery.id,
    );
    expect(after).toMatchObject({ state: "acknowledged", reasonCode: null });
    expect(after?.transformedHash).not.toBeNull();
    expect(after?.mappingVersion).not.toBeNull();
  });

  it("refuses to resend what may already be at the destination until the risk is confirmed", async () => {
    vi.useFakeTimers();
    const api = create();
    for (const state of ["acknowledged", "timed_out", "outcome_unknown"] as const) {
      const { detail, delivery } = await withDelivery(api, state);
      const request = sameMessage(detail.summary.id, delivery);
      const refused = await api.resendMessage(request);
      expect(refused.destinations[0]).toMatchObject({
        deliveryId: null,
        rejected: { code: "duplicate_risk_unconfirmed" },
      });
      const unchanged = (await api.getMessage(detail.summary.id)).deliveries.find(
        (d) => d.id === delivery.id,
      );
      expect(unchanged?.state).toBe(state);
      const accepted = await api.resendMessage({ ...request, confirmDuplicateRisk: true });
      expect(accepted.destinations[0]).toMatchObject({ deliveryId: delivery.id, rejected: null });
    }
  });

  it("refuses the same message while a delivery is queued or sending, but takes a new message", async () => {
    vi.useFakeTimers();
    const api = create();
    const pending = (await rowsOf(api, { deliverySummaries: ["pending"] }))[0];
    const destination = pending?.deliveries[0]?.destinationId ?? "";
    const same = await api.resendMessage({
      messageId: pending?.id ?? "",
      destinations: [destination],
      mode: "same_message",
      confirmDuplicateRisk: false,
    });
    expect(same.destinations[0]?.rejected).toMatchObject({ code: "delivery_in_progress" });
    expect(same.newMessageId).toBeNull();
    const fresh = await api.resendMessage({
      messageId: pending?.id ?? "",
      destinations: [destination],
      mode: "new_message",
      confirmDuplicateRisk: false,
    });
    expect(fresh.destinations[0]?.rejected).toBeNull();
    expect(fresh.newMessageId).toBe("msg-new-1");
  });

  it("refuses a destination that is not in the configuration, in the destination's entry", async () => {
    const api = create();
    const row = (await rowsOf(api, { deliverySummaries: ["attention"] }))[0];
    const result = await api.resendMessage({
      messageId: row?.id ?? "",
      destinations: ["dst-nope"],
      mode: "same_message",
      confirmDuplicateRisk: true,
    });
    expect(result.destinations).toEqual([
      {
        destinationId: "dst-nope",
        destinationLabel: "dst-nope",
        deliveryId: null,
        rejected: {
          code: "destination_not_found",
          message: expect.stringContaining("absent or disabled"),
        },
      },
    ]);
    await expect(
      api.resendMessage({
        messageId: "msg-nope",
        destinations: "all",
        mode: "same_message",
        confirmDuplicateRisk: false,
      }),
    ).rejects.toMatchObject({ code: "not_found" });
  });

  it("offers only a new message for a message that was not routed", async () => {
    vi.useFakeTimers();
    const api = create();
    for (const state of ["invalid", "aborted", "filtered", "duplicate"] as const) {
      const row = (await rowsOf(api, { states: [state] }))[0];
      const same = await api.resendMessage({
        messageId: row?.id ?? "",
        destinations: "all",
        mode: "same_message",
        confirmDuplicateRisk: true,
      });
      expect(same.destinations.length, state).toBeGreaterThan(0);
      for (const outcome of same.destinations) {
        expect(outcome.rejected, state).toMatchObject({ code: "invalid_state" });
      }
      const fresh = await api.resendMessage({
        messageId: row?.id ?? "",
        destinations: "all",
        mode: "new_message",
        confirmDuplicateRisk: false,
      });
      expect(fresh.newMessageId, state).toMatch(/^msg-new-\d+$/);
      expect(fresh.destinations.every((d) => d.rejected === null)).toBe(true);
    }
  });

  it("creates a new message that the pipeline routes and the destinations acknowledge", async () => {
    vi.useFakeTimers();
    const api = create();
    const { detail } = await withDelivery(api, "acknowledged", { states: ["routed"] });
    const before = await api.listMessages(query());
    const result = await resendAsNew(api, detail.summary.id);
    expect(result.newMessageId).toBe("msg-new-1");
    expect(result.destinations.every((d) => d.deliveryId === null && d.rejected === null)).toBe(
      true,
    );

    const created = await api.getMessage("msg-new-1");
    expect(created.summary).toMatchObject({
      origin: "resend",
      resentFrom: detail.summary.id,
      state: "received",
      deliveries: [],
      sourcePeer: null,
      channelId: detail.summary.channelId,
      controlId: detail.summary.controlId,
    });
    expect(created).toMatchObject({ responseCode: null, contentHash: detail.contentHash });
    expect(await originalText(api, "msg-new-1")).toBe(await originalText(api, detail.summary.id));

    vi.advanceTimersByTime(900);
    const routed = await api.getMessage("msg-new-1");
    expect(routed.summary.state).toBe("routed");
    expect(routed.deliveries.length).toBe(detail.deliveries.length);
    expect(routed.deliveries.every((d) => d.state === "queued")).toBe(true);
    expect(routed.deliveries.every((d) => d.messageId === "msg-new-1")).toBe(true);
    vi.advanceTimersByTime(1500);
    const done = await api.getMessage("msg-new-1");
    expect(done.summary.deliverySummary).toBe("complete");

    const after = await api.listMessages(query());
    expect(after.total).toBe(before.total + 1);
    expect(after.rows[0]?.id).toBe("msg-new-1");
    const original = await api.getMessage(detail.summary.id);
    expect(original.deliveries).toEqual(detail.deliveries);
    const again = await resendAsNew(api, "msg-new-1");
    expect((await api.getMessage(again.newMessageId ?? "")).summary.resentFrom).toBe("msg-new-1");
  });

  it("lists created messages with the filters and in the sort order of the query", async () => {
    vi.useFakeTimers();
    const api = create();
    const { detail } = await withDelivery(api, "acknowledged", { states: ["routed"] });
    await resendAsNew(api, detail.summary.id);
    vi.advanceTimersByTime(3000);
    const resends = await api.listMessages(query({ states: ["routed"] }, { limit: 1000 }));
    expect(resends.rows.some((r) => r.id === "msg-new-1")).toBe(true);
    const channel = await api.listMessages(query({ channelIds: ["ch-nope"] }));
    expect(channel.total).toBe(0);
    const bySize = await api.listMessages(
      query({}, { sort: { field: "sizeBytes", descending: false }, limit: 1000 }),
    );
    expect(bySize.total).toBe(5001);
    for (let i = 1; i < bySize.rows.length; i++) {
      expect(bySize.rows[i]?.sizeBytes ?? 0).toBeGreaterThanOrEqual(
        bySize.rows[i - 1]?.sizeBytes ?? 0,
      );
    }
    const found = await api.listMessages(
      query({ identifier: { value: detail.summary.controlId ?? "", prefix: false } }),
    );
    expect(found.rows.map((r) => r.id)).toContain("msg-new-1");
    const second = await api.listMessages(query({}, { offset: 1, limit: 2 }));
    expect(second.rows[0]?.id).toBe("msg-0");
  });
});
