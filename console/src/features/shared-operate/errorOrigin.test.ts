import { describe, expect, it } from "vitest";
import { isRetrying, resolveErrorOrigin } from "./errorOrigin";
import {
  ago,
  bridgeStatus,
  channel,
  destination,
  endpoint,
  messageChannel,
  side,
  sourceStatus,
} from "./testFixtures";

const retrying = destination({
  destinationId: "dst-backup",
  label: "Backup LIS",
  endpoint: { endpointId: "ep-backup", label: "Backup LIS link", state: "retrying" },
  nextAttemptAt: new Date(Date.now() + 20_000).toISOString(),
  lastError: { at: ago(30), summary: "connection refused", nextAction: "Check the backup LIS." },
});
const healthy = destination({
  destinationId: "dst-main",
  label: "Main LIS",
  endpoint: { endpointId: "ep-main", label: "Main LIS link", state: "connected" },
});
const endpoints = [
  endpoint({ id: "ep-backup", consecutiveFailureCount: 4, nextRetryAt: ago(-60) }),
  endpoint({ id: "ep-main" }),
];
const error = { at: ago(30), summary: "connection refused", nextAction: "Check the backup LIS." };

describe("resolveErrorOrigin", () => {
  it("finds the destination that reports the same error, with its endpoint's streak", () => {
    const origin = resolveErrorOrigin(messageChannel(), error, [healthy, retrying], endpoints);
    expect(origin).toMatchObject({
      kind: "destination",
      streak: 4,
      nextAttemptAt: retrying.nextAttemptAt,
    });
    expect(origin?.kind === "destination" && origin.destination.label).toBe("Backup LIS");
  });

  it("finds the failing destination while the source is healthy, when the summary is no help", () => {
    const origin = resolveErrorOrigin(
      messageChannel(),
      { ...error, summary: "something else" },
      [healthy, { ...retrying, lastError: null }],
      endpoints,
    );
    expect(origin?.kind).toBe("destination");
  });

  it("names the destination the engine's summary names", () => {
    const summary =
      'Channel ch-hl7 destination dst-main endpoint "Main LIS link" send failed: boom.';
    const origin = resolveErrorOrigin(
      messageChannel(),
      { ...error, summary },
      [retrying, healthy],
      endpoints,
    );
    expect(origin?.kind === "destination" && origin.destination.destinationId).toBe("dst-main");
  });

  it("falls back to the endpoint's next retry when the destination has no attempt scheduled", () => {
    const origin = resolveErrorOrigin(
      messageChannel(),
      error,
      [{ ...retrying, nextAttemptAt: null }],
      endpoints,
    );
    expect(origin).toMatchObject({ kind: "destination", nextAttemptAt: endpoints[0]?.nextRetryAt });
  });

  it("is the source when the summary says so, or when the source is down", () => {
    const summary = 'Channel ch-hl7 source endpoint "Chemistry source" accept failed: boom.';
    const down = messageChannel({
      source: sourceStatus({
        endpoint: side({ role: "source", state: "error", consecutiveFailureCount: 3 }),
      }),
    });
    expect(resolveErrorOrigin(down, { ...error, summary }, [retrying], endpoints)).toMatchObject({
      kind: "source",
      streak: 3,
    });
    const unrelated = { ...error, summary: "something else" };
    expect(resolveErrorOrigin(down, unrelated, [retrying], endpoints)).toMatchObject({
      kind: "source",
    });
  });

  it("uses the named side of a bridge, else its worst side", () => {
    const bridge = channel({
      bridge: bridgeStatus({
        endpointA: side({ consecutiveFailureCount: 2 }),
        endpointB: side({ role: "b", consecutiveFailureCount: 5 }),
      }),
    });
    const named = 'Channel ch-alpha A endpoint "Analyzer COM3" open failed: boom.';
    expect(resolveErrorOrigin(bridge, { ...error, summary: named }, [], [])).toEqual({
      kind: "bridge",
      streak: 2,
    });
    expect(resolveErrorOrigin(bridge, error, [], [])).toEqual({ kind: "bridge", streak: 5 });
  });
});

describe("isRetrying", () => {
  it("is a destination whose endpoint is down and whose attempt is scheduled", () => {
    expect(isRetrying(retrying)).toBe(true);
    expect(isRetrying({ ...retrying, enabled: false })).toBe(false);
    expect(isRetrying({ ...retrying, nextAttemptAt: null })).toBe(false);
    expect(isRetrying({ ...healthy, nextAttemptAt: ago(-5) })).toBe(false);
  });
});
