import { describe, expect, it } from "vitest";
import { parseErrorSummary } from "./errorSummary";

describe("parseErrorSummary", () => {
  it("splits the engine's error pattern into operation, endpoint and cause", () => {
    expect(
      parseErrorSummary(
        'Channel ch-1 A endpoint "Analyzer COM3" open failed: access denied. Close the program using COM3.',
      ),
    ).toEqual({
      role: "A",
      endpointLabel: "Analyzer COM3",
      operation: "open",
      cause: "access denied",
    });
    expect(
      parseErrorSummary(
        'Channel ch-2 destination dst-1 endpoint "LIS" send failed: connection refused. The engine retries.',
      )?.role,
    ).toBe("destination dst-1");
  });

  it("returns null for any other wording instead of guessing", () => {
    expect(parseErrorSummary("Buffer full: the LIS stopped reading")).toBeNull();
  });
});
