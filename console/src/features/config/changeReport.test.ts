import { describe, expect, it } from "vitest";
import type { ConfigDocument, ConfigVersionSummary } from "@/api/types";
import {
  buildChangeReport,
  COMPARISON_DIRECTION,
  changeReportCsv,
  changeReportJson,
  flattenComparison,
  type ReportVersion,
} from "./changeReport";
import { compareDocuments } from "./documentCompare";
import { sampleConfig } from "./testFixtures";

function summary(version: number, extra: Partial<ConfigVersionSummary> = {}): ConfigVersionSummary {
  return {
    version,
    appliedAt: `2026-11-0${version}T09:00:00Z`,
    appliedBy: "LAB\\jdoe",
    note: `note ${version}`,
    endpoints: 3,
    channels: 2,
    verifications: [],
    outcomes: [],
    current: false,
    ...extra,
  };
}

/** Version 1 is `sampleConfig`; version 2 renames the results channel, shortens its retention and raises a threshold; version 3 removes it. */
function history(): ReportVersion[] {
  const first: ConfigDocument = { ...sampleConfig(), version: 1 };
  const second = structuredClone(first);
  second.version = 2;
  const results = second.channels.find((channel) => channel.id === "results");
  if (results?.kind !== "message") throw new Error("fixture");
  results.label = "Results to LIS (renamed)";
  results.retention.days = 7;
  second.alertThresholds.queueDepthWarning = 750;
  const third: ConfigDocument = {
    ...second,
    version: 3,
    channels: second.channels.filter((channel) => channel.id !== "results"),
  };
  return [
    { summary: summary(1, { appliedBy: null, note: null }), document: first, previous: null },
    {
      summary: summary(2, {
        outcomes: [
          { channelId: "chem-bridge", outcome: "unchanged" },
          { channelId: "results", outcome: "updated" },
        ],
        verifications: [
          { by: "LAB\\mlopez", at: "2026-11-03T10:00:00Z", note: "VR-2026-018" },
          { by: "LAB\\jdoe", at: "2026-11-04T10:00:00Z", note: "=VR, second" },
        ],
      }),
      document: second,
      previous: first,
    },
    {
      summary: summary(3, {
        current: true,
        outcomes: [
          { channelId: "results", outcome: "failed", reason: "Cannot open COM24: access denied" },
        ],
      }),
      document: third,
      previous: second,
    },
  ];
}

const input = { generatedAt: "2026-11-07T08:00:00.000Z", generatedBy: "LAB\\jdoe" };

describe("flattenComparison", () => {
  it("gives each difference as field, before and after, naming the item and the setting", () => {
    const [first, second] = history();
    const changes = flattenComparison(
      compareDocuments(first?.document as ConfigDocument, second?.document as ConfigDocument),
    );
    expect(changes).toEqual([
      {
        field: "Channel Results to LIS (renamed) (results) › Label",
        before: "Results to LIS",
        after: "Results to LIS (renamed)",
      },
      {
        field: "Channel Results to LIS (renamed) (results) › Retention › Keep for (days)",
        before: "30",
        after: "7",
      },
      { field: "Alert thresholds › Delivery queue warning", before: "500", after: "750" },
    ]);
  });

  it("marks added and removed items", () => {
    const [, second, third] = history();
    expect(
      flattenComparison(
        compareDocuments(second?.document as ConfigDocument, third?.document as ConfigDocument),
      ),
    ).toEqual([
      { field: "Channel Results to LIS (renamed) (results)", before: "present", after: "removed" },
    ]);
  });
});

describe("buildChangeReport", () => {
  const report = buildChangeReport({ ...input, versions: history() });

  it("says who made it, when, and how to read the comparisons", () => {
    expect(report.generated_at).toBe(input.generatedAt);
    expect(report.generated_by).toBe("LAB\\jdoe");
    expect(report.comparison_direction).toBe(COMPARISON_DIRECTION);
    expect(COMPARISON_DIRECTION).toContain("before is the value in N-1");
  });

  it("records each version's account, time, note and every verification", () => {
    expect(report.versions.map((each) => each.version)).toEqual([1, 2, 3]);
    expect(report.versions[0]).toMatchObject({ applied_by: null, note: null, verifications: [] });
    expect(report.versions[1]).toMatchObject({
      applied_by: "LAB\\jdoe",
      applied_at: "2026-11-02T09:00:00Z",
      note: "note 2",
      verifications: [
        {
          verified_by: "LAB\\mlopez",
          verified_at: "2026-11-03T10:00:00Z",
          reference: "VR-2026-018",
        },
        { verified_by: "LAB\\jdoe", verified_at: "2026-11-04T10:00:00Z", reference: "=VR, second" },
      ],
    });
  });

  it("names channels by label, also one the version removed, and keeps a failure's reason", () => {
    expect(report.versions[1]?.outcomes).toEqual([
      {
        channel_id: "results",
        channel: "Results to LIS (renamed)",
        outcome: "updated",
        reason: null,
      },
      {
        channel_id: "chem-bridge",
        channel: "Chemistry bridge",
        outcome: "unchanged",
        reason: null,
      },
    ]);
    expect(report.versions[2]?.outcomes).toEqual([
      {
        channel_id: "results",
        channel: "Results to LIS (renamed)",
        outcome: "failed",
        reason: "Cannot open COM24: access denied",
      },
    ]);
  });

  it("compares each version with the one before; the oldest has no comparison", () => {
    expect(report.versions[0]?.comparison).toBeNull();
    expect(report.versions[1]?.comparison?.against).toBe(1);
    expect(report.versions[1]?.comparison?.changes).toHaveLength(3);
    expect(report.versions[2]?.comparison?.against).toBe(2);
  });
});

describe("changeReportJson", () => {
  it("is one document with the same facts, parseable", () => {
    const report = buildChangeReport({ ...input, versions: history() });
    const parsed = JSON.parse(changeReportJson(report));
    expect(parsed).toEqual(report);
    expect(Object.keys(parsed)).toEqual([
      "generated_at",
      "generated_by",
      "comparison_direction",
      "versions",
    ]);
  });
});

describe("changeReportCsv", () => {
  const report = buildChangeReport({ ...input, versions: history() });
  const lines = changeReportCsv(report).split("\r\n");
  const header = (lines[0] ?? "").split(",");

  it("is one flat sheet: a header, a report row, then rows by kind", () => {
    expect(header.slice(0, 6)).toEqual([
      "kind",
      "version",
      "applied_by",
      "applied_at",
      "note",
      "compared_with",
    ]);
    const kinds = lines.slice(1, -1).map((line) => line.split(",")[0]);
    expect(kinds[0]).toBe("report");
    expect(new Set(kinds)).toEqual(
      new Set(["report", "version", "outcome", "change", "verification"]),
    );
    expect(kinds.filter((kind) => kind === "version")).toHaveLength(3);
    expect(kinds.filter((kind) => kind === "verification")).toHaveLength(2);
    expect(kinds.filter((kind) => kind === "change")).toHaveLength(4);
    expect(kinds.filter((kind) => kind === "outcome")).toHaveLength(3);
  });

  it("states who generated it and the comparison direction in the report row", () => {
    expect(lines[1]).toContain("generated 2026-11-07T08:00:00.000Z by LAB\\jdoe");
    expect(lines[1]).toContain("before is the value in N-1");
  });

  it("writes the account, time, note, values and sign-offs, and guards formulas", () => {
    const text = lines.join("\n");
    expect(text).toContain("version,2,LAB\\jdoe,2026-11-02T09:00:00Z,note 2,1");
    expect(text).toContain("Retention › Keep for (days),30,7");
    expect(text).toContain(
      "verification,2,,,,,,,,,,,,LAB\\mlopez,2026-11-03T10:00:00Z,VR-2026-018",
    );
    // A reference that a spreadsheet would run as a formula is neutralised.
    expect(text).toContain(`"'=VR, second"`);
    expect(text).toContain("failed,Cannot open COM24: access denied");
  });
});
