import { describe, expect, it } from "vitest";
import type { ApplyResult, ChannelOutcome } from "@/api/types";
import { channelNamer, describeApply, sortedOutcomes, summarizeOutcomes } from "./outcomes";
import { sampleConfig } from "./testFixtures";

const OUTCOMES: ChannelOutcome[] = [
  { channelId: "a", outcome: "unchanged" },
  { channelId: "b", outcome: "restarted" },
  { channelId: "c", outcome: "failed", reason: "Cannot open COM24: access denied" },
  { channelId: "d", outcome: "restarted" },
  { channelId: "e", outcome: "updated" },
];

describe("summarizeOutcomes", () => {
  it("counts what was done, failures last, and leaves the unchanged out", () => {
    expect(summarizeOutcomes(OUTCOMES)).toBe("2 restarted, 1 updated, 1 failed");
  });

  it("says when nothing changed or nothing was recorded", () => {
    expect(summarizeOutcomes([{ channelId: "a", outcome: "unchanged" }])).toBe(
      "No channel changed",
    );
    expect(summarizeOutcomes([])).toBe("No outcomes recorded");
  });
});

describe("sortedOutcomes", () => {
  it("puts failures first, then the other outcomes in a fixed order", () => {
    expect(sortedOutcomes(OUTCOMES).map((each) => each.channelId)).toEqual([
      "c",
      "b",
      "d",
      "e",
      "a",
    ]);
  });
});

describe("channelNamer", () => {
  it("takes the label from the first document that has the channel, else the id", () => {
    const config = sampleConfig();
    const nameOf = channelNamer(undefined, config.channels);
    expect(nameOf("results")).toBe("Results to LIS");
    expect(nameOf("gone")).toBe("gone");
  });
});

describe("describeApply", () => {
  const nameOf = channelNamer(sampleConfig().channels);
  const heading = { done: "Applied", partial: "Applied, but not everywhere" };

  function result(outcomes: ChannelOutcome[]): ApplyResult {
    return {
      version: 9,
      outcomes,
      restartedChannelIds: [],
      unchangedChannelCount: 0,
      failures: outcomes.flatMap((each) =>
        each.outcome === "failed" ? [{ channelId: each.channelId, reason: each.reason ?? "" }] : [],
      ),
    };
  }

  it("lists each changed channel under its outcome and counts the unchanged", () => {
    const report = describeApply(
      result([
        { channelId: "chem-bridge", outcome: "restarted" },
        { channelId: "results", outcome: "updated" },
        { channelId: "x", outcome: "unchanged" },
      ]),
      nameOf,
      { ...heading, lead: "Saved as version 9." },
    );
    expect(report).toEqual({
      tone: "success",
      title: "Applied",
      description:
        "Saved as version 9. Restarted: Chemistry bridge. Updated: Results to LIS. 1 unchanged.",
    });
  });

  it("says no channel needed a change", () => {
    const report = describeApply(
      result([{ channelId: "x", outcome: "unchanged" }]),
      nameOf,
      heading,
    );
    expect(report.description).toBe("No channel needed a change. 1 unchanged.");
  });

  it("calls a failure out with its reason, the active version and the kept definition", () => {
    const report = describeApply(
      result([
        { channelId: "chem-bridge", outcome: "failed", reason: "Cannot open COM24: access denied" },
        { channelId: "results", outcome: "started" },
      ]),
      nameOf,
      heading,
    );
    expect(report.tone).toBe("warning");
    expect(report.title).toBe("Applied, but not everywhere");
    expect(report.description).toContain("Version 9 is active");
    expect(report.description).toContain("keeps the previous definition");
    expect(report.description).toContain("Chemistry bridge: Cannot open COM24: access denied");
    expect(report.description).toContain("Started: Results to LIS.");
  });
});
