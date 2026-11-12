import { describe, expect, it } from "vitest";
import type { ResendResult } from "@/ipc/generated";
import type { ResendRequest } from "../types";
import { delivery, engine, RESEND_ALL } from "./test/messageRecords";
import type { AnyRequest } from "./wire";

describe("messages through the engine", () => {
  describe("resend", () => {
    const outcome = (id: string, queued: boolean): ResendResult["destinations"][number] => ({
      destination_id: id,
      destination_label: id.toUpperCase(),
      delivery_id: queued ? `q-${id}` : null,
      rejected: queued
        ? null
        : {
            code: "duplicate_risk_unconfirmed",
            message: "Check the destination and confirm the risk of duplicate delivery.",
          },
    });

    const wire = {
      type: "resend",
      message_id: "msg-1",
      destinations: "all",
      mode: "same_message",
      confirm_duplicate_risk: false,
      confirm_superseded: false,
      reason: null,
    };

    it.each([
      ["as asked", {}, {}],
      [
        "to the listed destinations, with the risk confirmed",
        { destinations: ["a", "b"], mode: "new_message", confirmDuplicateRisk: true },
        {
          destinations: { destinations: ["a", "b"] },
          mode: "new_message",
          confirm_duplicate_risk: true,
        },
      ],
      [
        "with its reason trimmed",
        { reason: "  Destination was down  " },
        { reason: "Destination was down" },
      ],
      ["with a blank reason as none", { reason: "   " }, {}],
      [
        "with the confirmation that it is superseded",
        { confirmSuperseded: true },
        { confirm_superseded: true },
      ],
    ] as const)(
      "sends the request %s, with no confirmation the operator did not give",
      async (_name, asked, sent) => {
        const { bridge, api } = engine();
        bridge.request("resend", () => ({
          type: "resent",
          new_message_id: null,
          destinations: [outcome("a", true)],
        }));
        await api.resendMessage({ ...RESEND_ALL, ...asked } as ResendRequest);
        expect(bridge.sent("resend")).toEqual([{ ...wire, ...sent }]);
      },
    );

    it("returns the outcome of every destination, refusals included, without failing", async () => {
      const { bridge, api } = engine();
      bridge.request("resend", () => ({
        type: "resent",
        new_message_id: "msg-2",
        destinations: [outcome("a", true), outcome("b", false)],
      }));
      await expect(api.resendMessage({ ...RESEND_ALL, mode: "new_message" })).resolves.toEqual({
        newMessageId: "msg-2",
        destinations: [
          { destinationId: "a", destinationLabel: "A", deliveryId: "q-a", rejected: null },
          {
            destinationId: "b",
            destinationLabel: "B",
            deliveryId: null,
            rejected: {
              code: "duplicate_risk_unconfirmed",
              message: "Check the destination and confirm the risk of duplicate delivery.",
            },
          },
        ],
      });
    });
  });

  describe("a message that a newer revision supersedes", () => {
    it("is refused as a whole, with the engine's words and its reason", async () => {
      const { bridge, api } = engine();
      const words =
        "A newer revision of this message exists (msg-9, received 2026-11-06T13:00:00Z). Resending this older version may reissue a corrected result; confirm that it is intended to resend it anyway.";
      bridge.request("resend", () => {
        throw {
          kind: "ipc",
          error: { code: "rejected", message: words, field_errors: [], reason_code: "superseded" },
        };
      });
      await expect(api.resendMessage(RESEND_ALL)).rejects.toMatchObject({
        code: "conflict",
        message: words,
        rejection: "superseded",
      });
    });
  });

  describe("close", () => {
    it("closes one delivery with the reason and returns it as it now is", async () => {
      const { bridge, api } = engine();
      bridge.request("close_delivery", (request: AnyRequest) => ({
        type: "delivery_closed",
        delivery: delivery({
          delivery_id: request.type === "close_delivery" ? request.delivery_id : "",
          state: "cancelled",
          closed_reason: request.type === "close_delivery" ? request.reason : null,
        }),
      }));
      const closed = await api.closeDelivery("d-9", "resolved by phone");
      expect(closed).toMatchObject({
        id: "d-9",
        state: "cancelled",
        closedReason: "resolved by phone",
      });
      expect(bridge.sent("close_delivery")).toEqual([
        { type: "close_delivery", delivery_id: "d-9", reason: "resolved by phone" },
      ]);
    });

    it("reports the engine's refusal in its words", async () => {
      const { bridge, api } = engine();
      bridge.request("close_delivery", () => {
        throw {
          kind: "ipc",
          error: {
            code: "rejected",
            message: "A delivery that is sending cannot be closed.",
            field_errors: [],
            reason_code: "delivery_in_progress",
          },
        };
      });
      await expect(api.closeDelivery("d-1", "x")).rejects.toMatchObject({
        code: "conflict",
        message: "A delivery that is sending cannot be closed.",
      });
    });
  });
});
