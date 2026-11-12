import { describe, expect, it } from "vitest";
import { countdownText } from "./countdownText";

const NOW = Date.parse("2026-11-06T12:00:00.000Z");
const at = (seconds: number) => new Date(NOW + seconds * 1000).toISOString();

describe("countdownText", () => {
  it("counts seconds, minutes and hours", () => {
    expect(countdownText(at(5), NOW)).toBe("in 5 s");
    expect(countdownText(at(0.2), NOW)).toBe("in 1 s");
    expect(countdownText(at(125), NOW)).toBe("in 2 min 5 s");
    expect(countdownText(at(120), NOW)).toBe("in 2 min");
    expect(countdownText(at(3720), NOW)).toBe("in 1 h 2 min");
  });

  it("says now once the time has passed, and shows an unreadable time as it is", () => {
    expect(countdownText(at(0), NOW)).toBe("now");
    expect(countdownText(at(-30), NOW)).toBe("now");
    expect(countdownText("soon", NOW)).toBe("soon");
  });
});
