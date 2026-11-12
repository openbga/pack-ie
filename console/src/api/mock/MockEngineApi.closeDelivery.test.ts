import { describe, expect, it, vi } from "vitest";
import { mockApiFactory, resendAsNew, rowsOf, withDelivery } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

describe("closeDelivery", () => {
  it("closes a delivery that needs attention, with the reason, and the message stops needing it", async () => {
    const api = create();
    const { detail, delivery } = await withDelivery(api, "rejected");
    const closed = await api.closeDelivery(delivery.id, "  Confirmed by phone  ");
    expect(closed).toMatchObject({
      id: delivery.id,
      state: "cancelled",
      closedReason: "Confirmed by phone",
    });
    const after = await api.getMessage(detail.summary.id);
    expect(after.deliveries.find((d) => d.id === delivery.id)).toEqual(closed);
    const others = detail.deliveries.filter((d) => d.id !== delivery.id);
    const stillBad = others.some((d) =>
      ["failed", "rejected", "timed_out", "outcome_unknown"].includes(d.state),
    );
    expect(after.summary.deliverySummary).toBe(stillBad ? "attention" : "complete");
    const listed = await rowsOf(api, { deliverySummaries: ["attention"] });
    expect(listed.some((r) => r.id === detail.summary.id)).toBe(stillBad);
    await expect(api.closeDelivery(delivery.id, "again")).rejects.toMatchObject({
      code: "conflict",
      message: expect.stringContaining("already resolved"),
    });
  });

  it("closes a queued delivery, and says why it cannot close the others", async () => {
    const api = create();
    const queued = await withDelivery(api, "queued");
    await expect(api.closeDelivery(queued.delivery.id, "Not needed")).resolves.toMatchObject({
      state: "cancelled",
    });
    const sending = await withDelivery(api, "sending");
    await expect(api.closeDelivery(sending.delivery.id, "Not needed")).rejects.toMatchObject({
      code: "conflict",
      message: expect.stringContaining("sending"),
    });
    const done = await withDelivery(api, "acknowledged");
    await expect(api.closeDelivery(done.delivery.id, "Not needed")).rejects.toMatchObject({
      code: "conflict",
    });
  });

  it("requires a reason of 1 to 500 characters, and a delivery that exists", async () => {
    const api = create();
    const { delivery } = await withDelivery(api, "failed");
    for (const reason of ["", "   ", "x".repeat(501)]) {
      await expect(api.closeDelivery(delivery.id, reason)).rejects.toMatchObject({
        code: "conflict",
        message: expect.stringContaining("1-500"),
      });
    }
    await expect(api.closeDelivery("dlv-nope", "x")).rejects.toMatchObject({ code: "not_found" });
    await expect(api.closeDelivery("dlv-99999-1", "x")).rejects.toMatchObject({
      code: "not_found",
    });
    await expect(api.closeDelivery(delivery.id, "x".repeat(500))).resolves.toMatchObject({
      state: "cancelled",
    });
  });

  it("closes a delivery of a message an operator created", async () => {
    vi.useFakeTimers();
    const api = create();
    const { detail } = await withDelivery(api, "acknowledged", { states: ["routed"] });
    await resendAsNew(api, detail.summary.id);
    vi.advanceTimersByTime(900);
    const routed = await api.getMessage("msg-new-1");
    const first = routed.deliveries[0];
    const closed = await api.closeDelivery(first?.id ?? "", "Not needed");
    expect(closed).toMatchObject({ state: "cancelled", messageId: "msg-new-1" });
  });
});
