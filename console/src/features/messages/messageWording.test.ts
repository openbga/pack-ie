import { describe, expect, it } from "vitest";
import type { AckLevel, AttemptResult, DeliveryState, MessageState } from "@/api/types";
import {
  ackEvidence,
  ackLevelText,
  ackSentence,
  attemptResultText,
  canClose,
  canResend,
  chipBadgeText,
  chipStateText,
  chipText,
  DELIVERY_STATES,
  deliveryLabel,
  directionShortText,
  directionText,
  isUncertain,
  MESSAGE_STATE_MEANING,
  MESSAGE_STATES,
  mayHaveBeenProcessed,
  messageStateStatus,
  needsAttention,
  newMessageOnly,
  reasonText,
  reasonWithCode,
} from "./messageWording";

describe("acknowledgement wording (operator-console.md §6)", () => {
  it("words each level as the spec does", () => {
    expect(ackLevelText("handed_off")).toBe("Written to the destination. Not confirmed by it.");
    expect(ackLevelText("received")).toBe("Received by the destination. Processing not confirmed.");
    expect(ackLevelText("accepted")).toBe("Accepted by the destination's application.");
    expect(ackLevelText("none")).toBe("No acknowledgement was received.");
  });

  it.each(["none", "handed_off", "received"] as AckLevel[])(
    "never calls %s delivered or a success",
    (level) => {
      expect(ackLevelText(level)).not.toMatch(/deliver|success/i);
      const chip = { destinationLabel: "LIS", state: "acknowledged" as const, ackLevel: level };
      expect(chipText(chip)).not.toMatch(/deliver|success/i);
      expect(ackSentence({ state: "acknowledged", ackLevel: level })).not.toMatch(
        /deliver|success/i,
      );
    },
  );

  it("says what an acknowledgement proved on a chip, and what a delivery still waits for", () => {
    expect(chipStateText({ state: "acknowledged", ackLevel: "handed_off" })).toBe(
      "Acknowledged (written)",
    );
    expect(chipStateText({ state: "acknowledged", ackLevel: "accepted" })).toBe(
      "Acknowledged (accepted)",
    );
    expect(chipStateText({ state: "failed", ackLevel: "none" })).toBe("Failed");
    expect(ackSentence({ state: "queued", ackLevel: "none" })).toContain("yet");
    expect(ackSentence({ state: "timed_out", ackLevel: "received" })).toContain("Received by");
  });
});

describe("delivery labels (operator-console.md §2)", () => {
  it("uses the spec's label for every state", () => {
    const labels = Object.fromEntries(
      DELIVERY_STATES.map((state) => [state, deliveryLabel(state)]),
    );
    expect(labels).toEqual({
      queued: "Queued",
      sending: "Sending",
      acknowledged: "Acknowledged",
      rejected: "Rejected by destination",
      timed_out: "No acknowledgement in time, may have been delivered",
      outcome_unknown: "Outcome unknown, may have been delivered",
      failed: "Failed, not delivered",
      cancelled: "Closed",
    });
  });

  it("knows which deliveries need attention, may be at the destination, and can be closed", () => {
    const states: DeliveryState[] = [...DELIVERY_STATES];
    expect(states.filter(needsAttention)).toEqual([
      "rejected",
      "timed_out",
      "outcome_unknown",
      "failed",
    ]);
    expect(states.filter(mayHaveBeenProcessed)).toEqual([
      "acknowledged",
      "timed_out",
      "outcome_unknown",
    ]);
    expect(states.filter(canClose)).toEqual([
      "queued",
      "rejected",
      "timed_out",
      "outcome_unknown",
      "failed",
    ]);
  });

  it("words every attempt result, and an attempt still in flight", () => {
    const results: AttemptResult[] = [
      "acknowledged",
      "rejected",
      "write_failed",
      "send_timeout",
      "ack_timeout",
      "connection_lost_after_send",
      "engine_stopped",
      "transform_error",
    ];
    const words = results.map(attemptResultText);
    expect(new Set(words).size).toBe(results.length);
    expect(words.every((w) => w.length > 5)).toBe(true);
    expect(attemptResultText(null)).toBe("In progress");
  });
});

describe("message states", () => {
  it("has a label, a tone and a meaning for each of the eight", () => {
    expect(MESSAGE_STATES).toHaveLength(8);
    for (const state of MESSAGE_STATES) {
      expect(messageStateStatus(state).label.length).toBeGreaterThan(3);
      expect(MESSAGE_STATE_MEANING[state].length).toBeGreaterThan(20);
    }
    expect(messageStateStatus("invalid").tone).toBe("danger");
    expect(messageStateStatus("routed").tone).toBe("neutral");
  });

  it("offers a resend for messages the channel processed or tried to", () => {
    const resendable = MESSAGE_STATES.filter(canResend);
    expect(resendable).toEqual(["routed", "filtered", "duplicate", "invalid", "aborted"]);
    expect(MESSAGE_STATES.filter(newMessageOnly)).toEqual([
      "filtered",
      "duplicate",
      "invalid",
      "aborted",
    ]);
  });
});

describe("reason codes", () => {
  it("words the codes of the spec and shows one it does not know as it is", () => {
    expect(reasonText("transform_error")).toContain("mapping");
    expect(reasonText("source_filter")).toContain("filter");
    expect(reasonText("receive_timeout")).toContain("timeout");
    expect(reasonText("engine_restarted")).toContain("restarted");
    expect(reasonText("unparseable")).toContain("parsed");
    expect(reasonText("control_message")).toContain("instrument request");
    expect(reasonText("something_new")).toBe("Reason code something_new.");
    expect(reasonWithCode("parse_error")).toBe("The bytes could not be parsed. (parse_error)");
    expect(reasonWithCode("something_new")).toBe("Reason code something_new.");
  });

  it.each([
    "retry_limit_exceeded",
    "receive_timeout",
    "incomplete_record",
    "empty_transfer",
    "message_too_large",
    "connection_lost",
    "transport_error",
    "channel_stopped",
    "file_too_large",
    "startup_interrupted",
  ])("words the abort reason %s", (code) => {
    expect(reasonText(code)).not.toContain("Reason code");
  });
});

describe("types stay exhaustive", () => {
  it("covers every message state", () => {
    const all: MessageState[] = [
      "receiving",
      "received",
      "routed",
      "filtered",
      "duplicate",
      "invalid",
      "aborted",
      "observed",
    ];
    expect([...MESSAGE_STATES].sort()).toEqual([...all].sort());
  });
});

describe("what the grid shows in little room", () => {
  it("words a direction in a few characters, and the full wording is the title's", () => {
    expect(directionShortText("inbound")).toBe("Inbound");
    expect(directionShortText("a_to_b")).toBe("A to B");
    expect(directionShortText("b_to_a")).toBe("B to A");
    expect(directionText("a_to_b")).toBe("Side A to side B");
  });

  it("names an acknowledged chip by what the acknowledgement proved", () => {
    expect(chipBadgeText({ state: "acknowledged", ackLevel: "handed_off" })).toBe("Written");
    expect(chipBadgeText({ state: "acknowledged", ackLevel: "received" })).toBe("Received");
    expect(chipBadgeText({ state: "acknowledged", ackLevel: "accepted" })).toBe("Accepted");
    expect(chipBadgeText({ state: "acknowledged", ackLevel: "none" })).toBe("Acknowledged");
    expect(chipBadgeText({ state: "failed", ackLevel: "none" })).toBe("Failed");
    expect(chipBadgeText({ state: "outcome_unknown", ackLevel: "none" })).toBe("Outcome unknown");
  });
});

describe("closing a delivery whose outcome is uncertain", () => {
  it("is uncertain exactly for a timeout and an unknown outcome", () => {
    expect(DELIVERY_STATES.filter(isUncertain)).toEqual(["timed_out", "outcome_unknown"]);
  });

  it("quotes the last acknowledgement code, whichever attempt got it", () => {
    const attempt = (no: number, ackCode: string | null) => ({
      attemptNo: no,
      startedAt: "2026-11-06T12:00:00Z",
      endedAt: null,
      result: null,
      ackCode,
      ackControlId: null,
      errorCode: null,
      ackLevel: "none" as const,
    });
    expect(
      ackEvidence({
        ackLevel: "received",
        attemptLog: [attempt(1, "AE"), attempt(2, "AA"), attempt(3, null)],
      }),
    ).toBe("the last acknowledgement code was AA");
  });

  it("says what the level proved when there is no code, and that nothing came at level none", () => {
    expect(ackEvidence({ ackLevel: "none", attemptLog: [] })).toBe(
      "no acknowledgement was received",
    );
    for (const level of ["handed_off", "received", "accepted"] as AckLevel[]) {
      expect(ackEvidence({ ackLevel: level, attemptLog: [] })).not.toMatch(/deliver|success/i);
    }
  });
});
