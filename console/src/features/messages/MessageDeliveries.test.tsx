import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { attempt, delivery, detail, HL7_TEXT, messageBody, stubApi } from "./fixtures";
import { deliveryCard, findRecord } from "./messageRecordSetup";

describe("MessagePage", () => {
  describe("deliveries", () => {
    it("shows each destination with its state, acknowledgement, attempts and mapping version", async () => {
      await findRecord(
        stubApi({
          getMessage: async () =>
            detail({
              deliveries: [
                delivery("LIS", {
                  attemptLog: [
                    attempt(1, {
                      result: "ack_timeout",
                      ackCode: null,
                      errorCode: "ack_timeout",
                      ackLevel: "none",
                    }),
                    attempt(2),
                  ],
                  attempts: 2,
                  totalAttempts: 2,
                }),
              ],
            }),
        }),
      );
      const card = deliveryCard("LIS");
      expect(card.getAllByText("Acknowledged").length).toBeGreaterThan(0);
      expect(card.getByText("Accepted by the destination's application.")).toBeTruthy();
      expect(card.getByText("2 in this cycle, 2 in all")).toBeTruthy();
      expect(card.getByText("Configuration 7")).toBeTruthy();
      const attempts = within(card.getByRole("table", { name: "Attempts to LIS" }));
      expect(attempts.getAllByRole("columnheader").map((header) => header.textContent)).toEqual([
        "Attempt",
        "Started",
        "Ended",
        "Result",
        "Acknowledgement code",
        "ACK ID",
        "Error code",
      ]);
      expect(attempts.getByText("No acknowledgement in time")).toBeTruthy();
      expect(attempts.getByText("ack_timeout")).toBeTruthy();
      expect(attempts.getByText("AA")).toBeTruthy();
    });

    it.each([
      ["handed_off", "Written to the destination. Not confirmed by it."],
      ["received", "Received by the destination. Processing not confirmed."],
    ] as const)(
      "says only what a %s acknowledgement proved, never delivered or success",
      async (level, words) => {
        await findRecord(
          stubApi({
            getMessage: async () => detail({ deliveries: [delivery("LIS", { ackLevel: level })] }),
          }),
        );
        const card = deliveryCard("LIS");
        expect(card.getByText(words)).toBeTruthy();
        expect(card.getAllByText("Acknowledged").length).toBeGreaterThan(0);
        expect(screen.getByRole("article", { name: "LIS" }).textContent).not.toMatch(
          /deliver|success/i,
        );
      },
    );

    it("words the unresolved states as the spec does, with what was and was not received", async () => {
      await findRecord(
        stubApi({
          getMessage: async () =>
            detail({
              deliveries: [
                delivery("Rejected", { state: "rejected", ackLevel: "none" }),
                delivery("Timed", { state: "timed_out", ackLevel: "received" }),
                delivery("Unknown", { state: "outcome_unknown", ackLevel: "none" }),
                delivery("Failed", { state: "failed", ackLevel: "none" }),
                delivery("Closed", {
                  state: "cancelled",
                  ackLevel: "none",
                  closedReason: "Entered by hand",
                }),
              ],
            }),
        }),
      );
      expect(deliveryCard("Rejected").getByText("Rejected by destination")).toBeTruthy();
      expect(
        deliveryCard("Timed").getByText("No acknowledgement in time, may have been delivered"),
      ).toBeTruthy();
      expect(
        deliveryCard("Timed").getByText("Received by the destination. Processing not confirmed."),
      ).toBeTruthy();
      expect(
        deliveryCard("Unknown").getByText("Outcome unknown, may have been delivered"),
      ).toBeTruthy();
      expect(deliveryCard("Unknown").getByText("No acknowledgement was received.")).toBeTruthy();
      expect(deliveryCard("Failed").getByText("Failed, not delivered")).toBeTruthy();
      expect(deliveryCard("Closed").getAllByText("Closed").length).toBeGreaterThan(0);
      expect(deliveryCard("Closed").getByText("Entered by hand")).toBeTruthy();
      expect(screen.getByText("Needs attention")).toBeTruthy();
      expect(screen.getByText(/4 deliveries failed, were rejected/)).toBeTruthy();
    });

    it("words the reason code, and says a failed mapping left nothing to show", async () => {
      await findRecord(
        stubApi({
          getMessage: async () =>
            detail({
              deliveries: [
                delivery("LIS", {
                  state: "failed",
                  ackLevel: "none",
                  reasonCode: "transform_error",
                  attempts: 0,
                  totalAttempts: 0,
                  attemptLog: [],
                  transformedHash: null,
                  mappingVersion: null,
                }),
              ],
            }),
        }),
      );
      const card = deliveryCard("LIS");
      expect(card.getByText(/mapping could not be applied/)).toBeTruthy();
      expect(card.getByText(/No attempt was made: nothing was sent/)).toBeTruthy();
      expect(card.getByText(/the mapping failed, so no output was produced/)).toBeTruthy();
      expect(card.queryByRole("button", { name: /Show transformed/ })).toBeNull();
    });

    it("opens the transformed message of a delivery only on request", async () => {
      const api = stubApi({
        getMessageBody: async (request: { which: { kind: string }; as: string }) =>
          messageBody({
            which: { kind: "transformed", deliveryId: "d-LIS" },
            content: request.which.kind === "transformed" ? "<STX>OUT<ETX>" : HL7_TEXT,
          }),
      });
      const { user } = await findRecord(api);
      expect(api.getMessageBody).not.toHaveBeenCalled();
      await user.click(
        within(screen.getByRole("article", { name: "LIS" })).getByRole("button", {
          name: "Show transformed message to lis",
        }),
      );
      expect(api.getMessageBody).toHaveBeenCalledWith({
        messageId: "msg-000001",
        which: { kind: "transformed", deliveryId: "d-LIS" },
        as: "text",
      });
      expect(
        (await screen.findByRole("region", { name: "Transformed message to LIS as text" }))
          .textContent,
      ).toBe("<STX>OUT<ETX>");
    });

    it("shows a delivery in flight with its attempt that has not ended", async () => {
      await findRecord(
        stubApi({
          getMessage: async () =>
            detail({
              deliveries: [
                delivery("LIS", {
                  state: "sending",
                  ackLevel: "none",
                  attemptLog: [
                    attempt(1, { result: null, endedAt: null, ackCode: null, ackLevel: "none" }),
                  ],
                }),
                delivery("Archive", {
                  state: "queued",
                  ackLevel: "none",
                  attempts: 0,
                  totalAttempts: 0,
                  attemptLog: [],
                  nextAttemptAt: "2026-11-06T12:30:00Z",
                }),
              ],
            }),
        }),
      );
      const lis = deliveryCard("LIS");
      expect(lis.getByText("In progress")).toBeTruthy();
      expect(lis.getByText("Not yet")).toBeTruthy();
      const archive = deliveryCard("Archive");
      expect(archive.getByText("No attempt has started yet.")).toBeTruthy();
      expect(archive.getByText("Next attempt").nextElementSibling?.textContent).toMatch(/2026|11/);
    });
  });
});
