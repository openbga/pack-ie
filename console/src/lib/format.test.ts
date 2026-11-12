import { describe, expect, it } from "vitest";
import { formatTimestamp } from "./format";

describe("compact timestamps for grid cells", () => {
  it("drops the year and the zone, and the date for today", () => {
    const at = "2026-10-12T14:03:05Z";
    expect(formatTimestamp(at, "UTC", { compact: true })).toBe("10-12 14:03");
    expect(formatTimestamp(at, "UTC", { compact: true, seconds: true })).toBe("10-12 14:03:05");
    expect(
      formatTimestamp(at, "UTC", { compact: true, today: new Date("2026-10-12T20:00:00Z") }),
    ).toBe("14:03");
  });
});

describe("log times", () => {
  const at = "2026-11-06T12:04:22.123Z";

  it("shows the date and the time to the millisecond in the zone", () => {
    expect(formatTimestamp(at, "UTC", { millis: true, zone: false })).toBe(
      "2026-11-06 12:04:22.123",
    );
    expect(
      formatTimestamp("2026-11-06T12:04:22.007Z", "America/Chicago", { millis: true, zone: false }),
    ).toBe("2026-11-06 06:04:22.007");
  });

  it("names the zone by default", () => {
    expect(formatTimestamp(at, "America/Chicago", { millis: true })).toBe(
      "2026-11-06 06:04:22.123 CST",
    );
  });

  it("gives the time of day alone", () => {
    expect(formatTimestamp(at, "UTC", { timeOnly: true, seconds: true })).toBe("12:04:22");
    expect(formatTimestamp(at, "UTC", { timeOnly: true, millis: true })).toBe("12:04:22.123");
  });

  it("falls back to UTC for a zone it does not know, and shows an unreadable time as received", () => {
    expect(formatTimestamp(at, "Mars/Olympus", { millis: true })).toBe(
      "2026-11-06 12:04:22.123 UTC",
    );
    expect(formatTimestamp("not a time", "UTC", { millis: true })).toBe("not a time");
  });
});
