import { describe, expect, it } from "vitest";
import type { ChannelSnapshot } from "@/ipc/generated";
import { snapshotFromIpc } from "./snapshot";
import { IDS, richConfig } from "./test/configFixtures";
import {
  bridgePart,
  bridgeSnapshot,
  endpointSnapshot,
  engineSnapshot,
  flow,
  messageChannelSnapshot,
  messagePart,
} from "./test/snapshotFixtures";

const config = richConfig();

function channel(raw: ChannelSnapshot, withConfig = true) {
  const snapshot = engineSnapshot();
  snapshot.channels = [raw];
  const mapped = snapshotFromIpc(snapshot, withConfig ? config : null).channels[0];
  if (!mapped) throw new Error("no channel");
  return mapped;
}

describe("channels", () => {
  it.each([
    ["operator_paused", "paused"],
    ["disabled_by_config", "disabled"],
    ["removed_by_config", "replaced"],
    ["replaced_by_config", "replaced"],
    ["shutdown", "shutdown"],
    ["not_implemented", "not_implemented"],
    ["store_unavailable", "store_unavailable"],
  ] as [string, string][])("stop reason %s reads %s", (reason, expected) => {
    const mapped = channel(
      bridgeSnapshot({ state: "stopped", stop_reason: reason as never, allowed_actions: [] }),
    );
    expect(mapped.state).toBe("stopped");
    expect(mapped.stopReason).toBe(expected);
  });

  it("reads the actions the engine allows", () => {
    expect(channel(bridgeSnapshot()).allowedActions).toEqual(["pause", "restart"]);
    expect(
      channel(bridgeSnapshot({ state: "stopped", allowed_actions: ["resume"] })).allowedActions,
    ).toEqual(["resume"]);
    expect(channel(bridgeSnapshot({ allowed_actions: [] })).allowedActions).toEqual([]);
  });

  it("reads a bridge: counters, buffers, endpoints, tap", () => {
    const mapped = channel(
      bridgeSnapshot({
        next_retry_at: "2026-11-06T12:00:30.000Z",
        last_error: {
          at: "2026-11-06T11:59:00.000Z",
          summary: "connect failed.",
          next_action: "Retrying in 5s.",
        },
        recent_errors: [
          {
            at: "2026-11-06T11:59:00.000Z",
            summary: "connect failed.",
            next_action: "Retrying in 5s.",
          },
          { at: "2026-11-06T11:00:00.000Z", summary: "older", next_action: null },
        ],
      }),
    );
    expect(mapped).toMatchObject({
      id: IDS.bridge,
      label: "Analyzer to LIS",
      kind: "bridge",
      enabled: true,
      state: "running",
      stopReason: null,
      protocol: null,
      counters: { bytesIn: 100, bytesOut: 50, messagesIn: 4, messagesOut: 3, retries: 2 },
      configVersion: 7,
      instanceStartedAt: "2026-11-06T11:00:00.000Z",
      nextRetryAt: "2026-11-06T12:00:30.000Z",
      queue: null,
      source: null,
      lastActivityAt: "2026-11-06T12:00:04.000Z",
      lastError: {
        at: "2026-11-06T11:59:00.000Z",
        summary: "connect failed.",
        nextAction: "Retrying in 5s.",
      },
      tapActive: true,
    });
    expect(mapped.recentErrors).toEqual([
      { at: "2026-11-06T11:59:00.000Z", summary: "connect failed." },
      { at: "2026-11-06T11:00:00.000Z", summary: "older" },
    ]);
    expect(mapped.buffers).toEqual([
      {
        direction: "a_to_b",
        usedBytes: 1_048_576,
        capacityBytes: 2_097_152,
        segments: 3,
        maxSegments: 500,
        overflowing: false,
      },
      {
        direction: "b_to_a",
        usedBytes: 0,
        capacityBytes: 2_097_152,
        segments: 0,
        maxSegments: 500,
        overflowing: false,
      },
    ]);
    expect(mapped.endpoints).toEqual([
      { endpointId: IDS.serial, label: "Analyzer (COM3)", state: "connected" },
      { endpointId: IDS.client, label: "LIS HL7", state: "connected" },
    ]);
  });

  it("reads the bridge part", () => {
    const { bridge } = channel(bridgeSnapshot());
    expect(bridge).toEqual({
      sessionState: "bridged",
      connectStrategy: "receiver_then_sender",
      receiver: "a",
      endpointA: {
        endpointId: IDS.serial,
        label: "Analyzer (COM3)",
        kind: "serial",
        role: "a",
        state: "connected",
        consecutiveFailureCount: 0,
        activePeer: "COM3",
        lastActivityAt: "2026-11-06T12:00:01.000Z",
      },
      endpointB: {
        endpointId: IDS.client,
        label: "LIS HL7",
        kind: "tcp_client",
        role: "b",
        state: "connected",
        consecutiveFailureCount: 1,
        activePeer: null,
        lastActivityAt: "2026-11-06T12:00:04.000Z",
      },
      bytesAToB: 100,
      bytesBToA: 50,
      messagesAToB: 4,
      messagesBToA: 3,
      framingErrors: 1,
      tapDropped: 0,
      overflowCount: 2,
      tap: { framer: "hl7_mllp", recordMessages: true, transcriptViewers: 1 },
    });
  });

  it("adds the overflow episodes of both directions", () => {
    const { bridge } = channel(
      bridgeSnapshot({
        bridge: bridgePart({
          a_to_b: flow({ overflow_count: 2 }),
          b_to_a: flow({ overflow_count: 3 }),
        }),
      }),
    );
    expect(bridge?.overflowCount).toBe(5);
  });

  it("has no tap counts without a framer, and no viewer means the tap is not active", () => {
    const mapped = channel(
      bridgeSnapshot({
        bridge: bridgePart({
          a_to_b: flow({ bytes: 1 }),
          b_to_a: flow({ bytes: 2 }),
          tap: {
            framer: "none",
            record_messages: false,
            framing_errors: null,
            tap_dropped: null,
            transcript_viewers: 0,
          },
        }),
      }),
    );
    expect(mapped.bridge).toMatchObject({
      bytesAToB: 1,
      bytesBToA: 2,
      messagesAToB: null,
      messagesBToA: null,
      framingErrors: null,
      tapDropped: null,
      tap: { framer: "none", recordMessages: false, transcriptViewers: 0 },
    });
    expect(mapped.tapActive).toBe(false);
  });

  it("measures pressure by the fullest buffer against either limit", () => {
    expect(channel(bridgeSnapshot()).pressure).toBeCloseTo(0.5);
    const bySegments = bridgeSnapshot({
      bridge: bridgePart({
        a_to_b: flow({ buffered_segments: 450, buffered_bytes: 10 }),
      }),
    });
    expect(channel(bySegments).pressure).toBeCloseTo(0.9);
    const overflowing = bridgeSnapshot({
      bridge: bridgePart({
        a_to_b: flow({ buffered_segments: 1, buffered_bytes: 1, overflowing: true }),
      }),
    });
    expect(channel(overflowing).pressure).toBe(1);
    const over = bridgeSnapshot({
      bridge: bridgePart({ b_to_a: flow({ buffered_bytes: 50_000_000 }) }),
    });
    expect(channel(over).pressure).toBe(1);
  });

  it("falls back to the endpoint list for a channel that reports no detail", () => {
    const raw = engineSnapshot();
    raw.channels = [
      bridgeSnapshot({ bridge: null, message: null, endpoints: [IDS.serial, "gone"] }),
    ];
    raw.endpoints = [endpointSnapshot(IDS.serial, { state: "retrying" })];
    const mapped = snapshotFromIpc(raw, null).channels[0];
    expect(mapped?.endpoints).toEqual([
      { endpointId: IDS.serial, label: "Analyzer (COM3)", state: "retrying" },
    ]);
    expect(mapped?.buffers).toEqual([]);
    expect(mapped?.bridge).toBeNull();
  });

  it("summarises what each destination acknowledged, for a message channel only", () => {
    expect(channel(messageChannelSnapshot()).destinations).toEqual([
      {
        destinationId: IDS.hl7,
        label: "LIS (HL7)",
        enabled: true,
        acknowledged: 7,
        lastAcknowledgedAt: "2026-11-06T12:00:02.000Z",
      },
      {
        destinationId: IDS.csv,
        label: "Export CSV",
        enabled: false,
        acknowledged: 0,
        lastAcknowledgedAt: null,
      },
    ]);
    expect(channel(bridgeSnapshot()).destinations).toBeUndefined();
  });

  it("reads a message channel: source, queue sums, counters", () => {
    const mapped = channel(messageChannelSnapshot());
    expect(mapped).toMatchObject({
      kind: "message",
      protocol: "astm",
      buffers: [],
      bridge: null,
      queue: { depth: 201, needsAttention: 6, oldestQueuedAt: "2026-11-06T11:00:00.000Z" },
      counters: { bytesIn: 500, bytesOut: 740, messagesIn: 5, messagesOut: 7, retries: 0 },
      lastActivityAt: "2026-11-06T12:00:04.000Z",
      tapActive: false,
    });
    expect(mapped.source).toEqual({
      endpoint: {
        endpointId: IDS.listener,
        label: "Chemistry listener",
        kind: "tcp_listener",
        role: "source",
        state: "connected",
        consecutiveFailureCount: 0,
        activePeer: "10.0.0.31:50123",
        lastActivityAt: "2026-11-06T12:00:03.000Z",
      },
      protocol: "astm",
      session: "receiving",
      lastReceivedAt: "2026-11-06T12:00:03.000Z",
      received: 5,
      duplicates: 1,
      invalid: 2,
      filtered: 3,
      aborted: 4,
    });
    expect(mapped.endpoints.map((e) => e.endpointId)).toEqual([
      IDS.listener,
      IDS.client,
      IDS.folderOut,
    ]);
  });

  it("measures a message channel's pressure by queue depth against the backlog threshold", () => {
    expect(channel(messageChannelSnapshot()).pressure).toBeCloseTo(201 / 500);
    expect(channel(messageChannelSnapshot(), false).pressure).toBeCloseTo(201 / 1000);
    const heavy = messageChannelSnapshot();
    const first = heavy.message?.destinations[0];
    if (first) first.queue.queued = 100_000;
    expect(channel(heavy).pressure).toBe(1);
  });

  it("has no queue pressure when the backlog alert is off", () => {
    const off = { ...config, alerts: { ...config.alerts, delivery_backlog_count: 0 } };
    const snapshot = engineSnapshot();
    snapshot.channels = [messageChannelSnapshot()];
    expect(snapshotFromIpc(snapshot, off).channels[0]?.pressure).toBe(0);
  });

  it("counts the unresolved deliveries the channel reports", () => {
    const raw = messageChannelSnapshot({
      message: messagePart({
        unresolved: {
          rejected: 1,
          timed_out: 2,
          outcome_unknown: 3,
          failed: 4,
          transform_errors: 0,
        },
      }),
    });
    expect(channel(raw).queue?.needsAttention).toBe(10);
  });

  it("keeps the next action of the last error", () => {
    const mapped = channel(
      bridgeSnapshot({
        last_error: { at: "2026-11-06T11:59:00.000Z", summary: "boom", next_action: "Wait" },
      }),
    );
    expect(mapped.lastError).toEqual({
      at: "2026-11-06T11:59:00.000Z",
      summary: "boom",
      nextAction: "Wait",
    });
    expect(
      channel(
        bridgeSnapshot({
          last_error: { at: "2026-11-06T11:59:00.000Z", summary: "boom", next_action: null },
        }),
      ).lastError?.nextAction,
    ).toBe("");
  });
});
