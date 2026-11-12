import { describe, expect, it } from "vitest";
import type { EngineSnapshot } from "@/ipc/generated";
import { destinationFrom } from "./destinations";
import realBridge from "./real/snapshot.json";
import realAlerts from "./real/snapshot-alerts.json";
import { snapshotFromIpc } from "./snapshot";

// What the running engine said (`packie status --json`), trimmed of nothing but its identity.
describe("a snapshot the real engine produced", () => {
  const bridgeFixture = realBridge as unknown as EngineSnapshot;
  const alertFixture = realAlerts as unknown as EngineSnapshot;

  it("maps a running bridge", () => {
    const snapshot = snapshotFromIpc(bridgeFixture, null);
    expect(snapshot.node).toMatchObject({
      nodeId: "node-1",
      state: "running",
      hosting: "console",
      configVersion: 1,
      pendingConfigVersion: null,
      store: {
        state: "not_configured",
        messageCount: null,
        databaseBytes: null,
        diskTotalBytes: null,
      },
      totals: { deliveriesUnresolved: 0, deliveriesQueued: 0, alertsWarning: 0, alertsCritical: 0 },
    });
    expect(snapshot.node.store?.diskFreeBytes).toBeGreaterThan(0);
    const [channel] = snapshot.channels;
    expect(channel).toMatchObject({
      id: "bridge-1",
      label: "Instrument to system",
      kind: "bridge",
      state: "running",
      stopReason: null,
      allowedActions: ["pause", "restart"],
      counters: { bytesIn: 3447, bytesOut: 3447, messagesIn: 0, messagesOut: 0, retries: 0 },
      tapActive: false,
      pressure: 0,
      lastError: {
        summary: expect.stringContaining("connection refused"),
        nextAction: "Retrying in 500 ms.",
      },
    });
    expect(channel?.bridge).toMatchObject({
      sessionState: "bridged",
      connectStrategy: "parallel",
      receiver: null,
      endpointA: {
        kind: "tcp_listener",
        role: "a",
        state: "connected",
        activePeer: "127.0.0.1:52386",
      },
      endpointB: { kind: "tcp_client", role: "b", activePeer: "127.0.0.1:5302" },
      bytesAToB: 3447,
      messagesAToB: null,
      framingErrors: null,
      tap: { framer: "none", recordMessages: false, transcriptViewers: 0 },
    });
    expect(channel?.buffers.map((b) => [b.direction, b.capacityBytes, b.maxSegments])).toEqual([
      ["a_to_b", 10_485_760, 1000],
      ["b_to_a", 10_485_760, 1000],
    ]);
    expect(snapshot.endpoints.map((e) => [e.id, e.summary, e.heldBy, e.activePeer])).toEqual([
      ["inst-side", "127.0.0.1:5301", "bridge-1", "127.0.0.1:52386"],
      ["sys-side", "127.0.0.1:5302", null, "127.0.0.1:5302"],
    ]);
    expect(snapshot.openAlerts).toEqual([]);
  });

  it("maps a message channel the engine has not started, and an alert", () => {
    const snapshot = snapshotFromIpc(alertFixture, null);
    const message = snapshot.channels.find((c) => c.kind === "message");
    expect(message).toMatchObject({
      state: "stopped",
      stopReason: "not_implemented",
      allowedActions: [],
      protocol: "hl7_mllp",
      queue: { depth: 0, needsAttention: 0, oldestQueuedAt: null },
      source: { protocol: "hl7_mllp", session: "closed", received: 0 },
    });
    expect(message?.endpoints.map((e) => e.endpointId)).toEqual(["hl7-in", "out-dir"]);
    const rawDestination = alertFixture.channels.find((c) => c.kind === "message")?.message
      ?.destinations[0];
    if (rawDestination === undefined) throw new Error("fixture");
    expect(destinationFrom(rawDestination)).toEqual({
      destinationId: "csv",
      label: "CSV",
      endpoint: { endpointId: "out-dir", label: "Result files", state: "disconnected" },
      protocol: "delimited_file",
      queued: 0,
      sending: 0,
      oldestQueuedAt: null,
      unresolved: { rejected: 0, failed: 0, timed_out: 0, outcome_unknown: 0 },
      transformErrors: 0,
      counters: {
        attempts: 0,
        acknowledged: 0,
        rejected: 0,
        timedOut: 0,
        outcomeUnknown: 0,
        failed: 0,
        bytesOut: 0,
      },
      lastAcknowledgedAt: null,
      lastAttemptAt: null,
      nextAttemptAt: null,
      lastError: null,
      enabled: true,
    });
    const retrying = snapshot.channels.find((c) => c.id === "br2");
    expect(retrying).toMatchObject({ state: "retrying", allowedActions: ["pause", "restart"] });
    expect(snapshot.openAlerts).toHaveLength(1);
    expect(snapshot.openAlerts[0]).toMatchObject({
      kind: "endpoint_down",
      severity: "warning",
      channelId: "br2",
      endpointId: "dead",
      state: "open",
      occurrences: 1,
      detail: "failures: 4, seconds: 0, state: retrying",
    });
    expect(snapshot.node.totals?.alertsWarning).toBe(1);
  });
});
