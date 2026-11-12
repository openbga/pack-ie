import { describe, expect, it } from "vitest";
import type { TransmissionReport as RawReport } from "@/ipc/generated";
import { transmissionFromIpc, transmissionToIpc } from "./reports";

const HASH = "ab".repeat(32);

function raw(extra: Partial<RawReport> = {}): RawReport {
  return {
    channel_id: "ch-1",
    since: "2026-11-06T00:00:00Z",
    until: "2026-11-07T00:00:00Z",
    generated_at: "2026-11-07T08:30:00Z",
    messages: 3,
    destinations: [
      {
        destination_id: "dest-lis",
        label: "LIS (HL7)",
        deliveries: 3,
        accepted: 1,
        received: 0,
        handed_off: 1,
        unresolved: 1,
        closed: 0,
        pending: 0,
      },
    ],
    rows: [
      {
        message_id: "msg-1",
        received_at: "2026-11-06T09:00:00Z",
        message_state: "routed",
        control_id: "C-1",
        source_endpoint_id: "ep-analyzer",
        source_peer: "10.0.4.17:41234",
        config_version: 12,
        mapping_version: 11,
        content_hash: HASH,
        destination_id: "dest-lis",
        state: "acknowledged",
        ack_level: "accepted",
        ack_code: "AA",
        ack_control_id: "ACK-C1",
        sent_hash: "cd".repeat(32),
        attempts: 1,
        first_attempt_at: "2026-11-06T09:00:01Z",
        acknowledged_at: "2026-11-06T09:00:02Z",
      },
      {
        message_id: "msg-2",
        received_at: "2026-11-06T10:00:00Z",
        message_state: "routed",
        control_id: null,
        source_endpoint_id: "ep-analyzer",
        source_peer: null,
        config_version: 12,
        mapping_version: null as unknown as number,
        content_hash: HASH,
        destination_id: "dest-lis",
        state: "timed_out",
        ack_level: "none",
        ack_code: null,
        ack_control_id: null,
        sent_hash: null,
        attempts: 3,
        first_attempt_at: "2026-11-06T10:00:01Z",
        acknowledged_at: null,
      },
    ],
    truncated: false,
    rows_limit: 10_000,
    period_rows_total: 5,
    ...extra,
  };
}

describe("transmissionToIpc", () => {
  it("names the channel and period, and leaves the end for the engine to default", () => {
    expect(transmissionToIpc({ channelId: "ch-1", since: "2026-11-06T00:00:00Z" })).toEqual({
      channel_id: "ch-1",
      since: "2026-11-06T00:00:00Z",
      until: null,
    });
    expect(
      transmissionToIpc({
        channelId: "ch-1",
        since: "2026-11-06T00:00:00Z",
        until: "2026-11-07T00:00:00Z",
      }),
    ).toEqual({
      channel_id: "ch-1",
      since: "2026-11-06T00:00:00Z",
      until: "2026-11-07T00:00:00Z",
    });
  });
});

describe("transmissionFromIpc", () => {
  it("reads the summary, the totals of each destination and every field of each row", () => {
    const report = transmissionFromIpc(raw());
    expect(report).toMatchObject({
      channelId: "ch-1",
      since: "2026-11-06T00:00:00Z",
      until: "2026-11-07T00:00:00Z",
      generatedAt: "2026-11-07T08:30:00Z",
      messages: 3,
      truncated: false,
    });
    expect(report.destinations).toEqual([
      {
        destinationId: "dest-lis",
        label: "LIS (HL7)",
        deliveries: 3,
        accepted: 1,
        received: 0,
        handedOff: 1,
        unresolved: 1,
        closed: 0,
        pending: 0,
      },
    ]);
    expect(report.rows[0]).toEqual({
      messageId: "msg-1",
      receivedAt: "2026-11-06T09:00:00Z",
      messageState: "routed",
      controlId: "C-1",
      sourceEndpointId: "ep-analyzer",
      sourcePeer: "10.0.4.17:41234",
      configVersion: 12,
      mappingVersion: 11,
      contentHash: HASH,
      destinationId: "dest-lis",
      state: "acknowledged",
      ackLevel: "accepted",
      ackCode: "AA",
      ackControlId: "ACK-C1",
      sentHash: "cd".repeat(32),
      attempts: 1,
      firstAttemptAt: "2026-11-06T09:00:01Z",
      acknowledgedAt: "2026-11-06T09:00:02Z",
    });
  });

  it("keeps what is missing missing, and the order of the rows", () => {
    const report = transmissionFromIpc(raw({ truncated: true, period_rows_total: 51_651 }));
    expect(report.truncated).toBe(true);
    // How complete the rows are: the limit, and the deliveries of the whole period.
    expect(report).toMatchObject({ rowsLimit: 10_000, periodRowsTotal: 51_651 });
    expect(report.rows.map((row) => row.messageId)).toEqual(["msg-1", "msg-2"]);
    expect(report.rows[1]).toMatchObject({
      controlId: null,
      sourcePeer: null,
      mappingVersion: null,
      ackCode: null,
      ackControlId: null,
      sentHash: null,
      firstAttemptAt: "2026-11-06T10:00:01Z",
      acknowledgedAt: null,
      ackLevel: "none",
      state: "timed_out",
    });
  });

  it("reads a report of an empty period", () => {
    const report = transmissionFromIpc(raw({ messages: 0, destinations: [], rows: [] }));
    expect(report).toMatchObject({ messages: 0, destinations: [], rows: [] });
  });
});
