import { describe, expect, it } from "vitest";
import type { DeliveryState, MessageState } from "@/api/types";
import { chip, delivery, detail, summary } from "./fixtures";
import {
  blockedDestinations,
  defaultDestinations,
  needsRiskConfirmation,
  needsSupersededConfirmation,
  offeredModes,
  type ResendSubject,
  subjectOfDetail,
  subjectOfSummary,
} from "./resendRules";

function subject(
  destinations: [string, DeliveryState][],
  state: MessageState = "routed",
): ResendSubject {
  return {
    messageId: "m",
    identifier: "C-1",
    channelLabel: "Chem",
    state,
    destinations: destinations.map(([id, deliveryState]) => ({
      id,
      label: id.toUpperCase(),
      state: deliveryState,
    })),
  };
}

describe("the resend subject", () => {
  it("reads the chips of a grid row, with the identifier people use", () => {
    const row = summary(1, {
      controlId: null,
      sampleId: "SMP-1",
      deliveries: [chip("LIS", { state: "failed", ackLevel: "none" })],
    });
    expect(subjectOfSummary(row)).toEqual({
      messageId: "msg-000001",
      identifier: "SMP-1",
      channelLabel: "Synthetic chemistry",
      state: "routed",
      destinations: [{ id: "dest-LIS", label: "LIS", state: "failed" }],
    });
    expect(subjectOfSummary(summary(2, { controlId: null, sampleId: null })).identifier).toBe(
      "msg-000002",
    );
  });

  it("reads the deliveries of a record", () => {
    const record = detail({ deliveries: [delivery("LIS", { state: "timed_out" })] });
    expect(subjectOfDetail(record).destinations).toEqual([
      { id: "dest-LIS", label: "LIS", state: "timed_out" },
    ]);
  });
});

describe("modes", () => {
  it("offers both for a routed message, and only a new message for the others", () => {
    expect(offeredModes(subject([["a", "failed"]]))).toEqual(["same_message", "new_message"]);
    for (const state of ["invalid", "aborted", "filtered", "duplicate"] as const) {
      expect(offeredModes(subject([], state)), state).toEqual(["new_message"]);
      expect(offeredModes(subject([["a", "failed"]], state)), state).toEqual(["new_message"]);
    }
    expect(offeredModes(subject([]))).toEqual(["new_message"]);
  });
});

describe("destinations", () => {
  it("starts with the deliveries that need attention", () => {
    const s = subject([
      ["a", "acknowledged"],
      ["b", "rejected"],
      ["c", "failed"],
      ["d", "timed_out"],
      ["e", "outcome_unknown"],
      ["f", "queued"],
      ["g", "cancelled"],
    ]);
    expect(defaultDestinations(s)).toEqual(["b", "c", "d", "e"]);
    expect(defaultDestinations(subject([["a", "acknowledged"]]))).toEqual([]);
  });

  it("blocks a delivery in progress for the same message only", () => {
    const s = subject([
      ["a", "queued"],
      ["b", "sending"],
      ["c", "failed"],
    ]);
    expect(blockedDestinations(s, ["a", "b", "c"], "same_message")).toEqual(["a", "b"]);
    expect(blockedDestinations(s, ["c"], "same_message")).toEqual([]);
    expect(blockedDestinations(s, ["a", "b", "c"], "new_message")).toEqual([]);
  });
});

describe("the duplicate-risk confirmation", () => {
  it.each([
    ["queued", false],
    ["sending", false],
    ["failed", false],
    ["rejected", false],
    ["cancelled", false],
    ["acknowledged", true],
    ["timed_out", true],
    ["outcome_unknown", true],
  ] as const)("a selected %s delivery needs it: %s", (state, needed) => {
    expect(needsRiskConfirmation(subject([["a", state]]), ["a"])).toBe(needed);
  });

  it("only looks at the selected destinations", () => {
    const s = subject([
      ["a", "acknowledged"],
      ["b", "failed"],
    ]);
    expect(needsRiskConfirmation(s, ["b"])).toBe(false);
    expect(needsRiskConfirmation(s, ["a", "b"])).toBe(true);
    expect(needsRiskConfirmation(s, [])).toBe(false);
  });
});

describe("needsSupersededConfirmation", () => {
  it("is needed when the record names a newer revision, or the engine refused for one", () => {
    expect(needsSupersededConfirmation("msg-9", false)).toBe(true);
    expect(needsSupersededConfirmation(null, true)).toBe(true);
    expect(needsSupersededConfirmation("msg-9", true)).toBe(true);
    expect(needsSupersededConfirmation(null, false)).toBe(false);
  });
});
