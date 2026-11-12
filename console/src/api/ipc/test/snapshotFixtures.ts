/**
 * Test data: an engine snapshot and its parts in the generated types (docs/spec/diagnostics.md),
 * consistent with `richConfig()`.
 */

import type {
  Alert,
  BridgeDetail,
  ChannelSnapshot,
  DestinationSnapshot,
  EndpointRef,
  EndpointSnapshot,
  EngineSnapshot,
  FlowSnapshot,
  MessageDetail,
  TrafficSnapshot,
  Unresolved,
} from "@/ipc/generated";
import { IDS, richConfig } from "./configFixtures";

const AT = "2026-11-06T12:00:05.000Z";

const NO_UNRESOLVED: Unresolved = {
  rejected: 0,
  timed_out: 0,
  outcome_unknown: 0,
  failed: 0,
  transform_errors: 0,
};

const NO_DELIVERIES: DestinationSnapshot["counters"] = {
  attempts: 0,
  acknowledged: 0,
  rejected: 0,
  timed_out: 0,
  outcome_unknown: 0,
  failed: 0,
  bytes_out: 0,
};

function traffic(extra: Partial<TrafficSnapshot> = {}): TrafficSnapshot {
  return {
    bytes_in: 0,
    bytes_out: 0,
    messages_in: 0,
    messages_out: 0,
    errors: 0,
    last_activity: null,
    ...extra,
  };
}

function endpointRef(id: string, role: string, extra: Partial<EndpointRef> = {}): EndpointRef {
  const config = richConfig().endpoints?.find((e) => e.endpoint_id === id);
  return {
    endpoint_id: id,
    label: config?.label ?? id,
    kind: config?.kind ?? "serial",
    role,
    state: "connected",
    consecutive_failures: 0,
    active_peer: null,
    last_activity: null,
    ...extra,
  };
}

export function endpointSnapshot(
  id: string,
  extra: Partial<EndpointSnapshot> = {},
): EndpointSnapshot {
  const config = richConfig().endpoints?.find((e) => e.endpoint_id === id);
  return {
    endpoint_id: id,
    label: config?.label ?? id,
    kind: config?.kind ?? "serial",
    enabled: true,
    state: "connected",
    state_since: "2026-11-06T11:00:00.000Z",
    peer: null,
    held_by: null,
    settings_summary: config ? `${config.label} summary` : "",
    consecutive_failures: 0,
    retry_count: 0,
    next_retry_at: null,
    traffic: traffic({
      bytes_in: 10,
      bytes_out: 20,
      messages_in: 1,
      messages_out: 2,
      last_activity: AT,
    }),
    last_error: null,
    rejections: null,
    channels: [],
    ...extra,
  };
}

export function flow(extra: Partial<FlowSnapshot> = {}): FlowSnapshot {
  return {
    bytes: 0,
    messages: null,
    buffered_segments: 0,
    buffered_bytes: 0,
    max_segments: 500,
    max_bytes: 2_097_152,
    overflowing: false,
    overflow_count: 0,
    ...extra,
  };
}

export function bridgePart(extra: Partial<BridgeDetail> = {}): BridgeDetail {
  return {
    session: "bridged",
    session_since: "2026-11-06T11:00:01.000Z",
    connect_strategy: "receiver_then_sender",
    receiver: "a",
    endpoint_a: endpointRef(IDS.serial, "a", {
      active_peer: "COM3",
      last_activity: "2026-11-06T12:00:01.000Z",
    }),
    endpoint_b: endpointRef(IDS.client, "b", {
      consecutive_failures: 1,
      last_activity: "2026-11-06T12:00:04.000Z",
    }),
    a_to_b: flow({
      bytes: 100,
      messages: 4,
      buffered_segments: 3,
      buffered_bytes: 1_048_576,
      overflow_count: 2,
    }),
    b_to_a: flow({ bytes: 50, messages: 3 }),
    tap: {
      framer: "hl7_mllp",
      record_messages: true,
      framing_errors: 1,
      tap_dropped: 0,
      transcript_viewers: 1,
    },
    ...extra,
  };
}

export function bridgeSnapshot(extra: Partial<ChannelSnapshot> = {}): ChannelSnapshot {
  return {
    channel_id: IDS.bridge,
    label: "Analyzer to LIS",
    kind: "bridge",
    enabled: true,
    state: "running",
    stop_reason: null,
    state_since: "2026-11-06T11:00:01.000Z",
    config_version: 7,
    instance_started_at: "2026-11-06T11:00:00.000Z",
    endpoints: [IDS.serial, IDS.client],
    traffic: traffic({
      bytes_in: 100,
      bytes_out: 50,
      messages_in: 4,
      messages_out: 3,
      last_activity: "2026-11-06T12:00:04.000Z",
    }),
    retry_count: 2,
    next_retry_at: null,
    last_error: null,
    recent_errors: [],
    allowed_actions: ["pause", "restart"],
    bridge: bridgePart(),
    message: null,
    ...extra,
  };
}

export function destinationSnapshot(
  id: string,
  endpoint: string,
  extra: Partial<DestinationSnapshot> = {},
): DestinationSnapshot {
  return {
    destination_id: id,
    label: id,
    enabled: true,
    endpoint: endpointRef(endpoint, id),
    protocol: "hl7_mllp",
    queue: { queued: 0, sending: 0, oldest_queued_at: null, next_attempt_at: null },
    unresolved: NO_UNRESOLVED,
    counters: NO_DELIVERIES,
    last_attempt_at: null,
    last_ack_at: null,
    last_error: null,
    ...extra,
  };
}

export function messagePart(extra: Partial<MessageDetail> = {}): MessageDetail {
  return {
    source: endpointRef(IDS.listener, "source", {
      active_peer: "10.0.0.31:50123",
      last_activity: "2026-11-06T12:00:03.000Z",
    }),
    protocol: "astm",
    session: "receiving",
    counters: { received: 5, duplicates: 1, invalid: 2, filtered: 3, aborted: 4, bytes_in: 500 },
    last_received_at: "2026-11-06T12:00:03.000Z",
    destinations: [
      destinationSnapshot(IDS.hl7, IDS.client, {
        label: "LIS (HL7)",
        queue: {
          queued: 200,
          sending: 1,
          oldest_queued_at: "2026-11-06T11:30:00.000Z",
          next_attempt_at: "2026-11-06T12:00:30.000Z",
        },
        unresolved: { ...NO_UNRESOLVED, rejected: 2 },
        counters: { ...NO_DELIVERIES, attempts: 9, acknowledged: 7, rejected: 2, bytes_out: 700 },
        last_attempt_at: "2026-11-06T12:00:04.000Z",
        last_ack_at: "2026-11-06T12:00:02.000Z",
        last_error: {
          at: "2026-11-06T12:00:04.000Z",
          summary: "Channel ch-message destination d-hl7 send failed.",
          next_action: "Retrying in 5s.",
        },
      }),
      destinationSnapshot(IDS.csv, IDS.folderOut, {
        label: "Export CSV",
        enabled: false,
        protocol: "delimited_file",
        queue: {
          queued: 0,
          sending: 0,
          oldest_queued_at: "2026-11-06T11:00:00.000Z",
          next_attempt_at: null,
        },
        unresolved: { ...NO_UNRESOLVED, outcome_unknown: 1, failed: 3 },
      }),
    ],
    unresolved: { ...NO_UNRESOLVED, rejected: 2, outcome_unknown: 1, failed: 3 },
    ...extra,
  };
}

export function messageChannelSnapshot(extra: Partial<ChannelSnapshot> = {}): ChannelSnapshot {
  return {
    ...bridgeSnapshot(),
    channel_id: IDS.message,
    label: "Chemistry results",
    kind: "message",
    endpoints: [IDS.listener, IDS.client, IDS.folderOut],
    traffic: traffic({
      bytes_in: 500,
      bytes_out: 740,
      messages_in: 5,
      messages_out: 7,
      last_activity: "2026-11-06T12:00:04.000Z",
    }),
    retry_count: 0,
    bridge: null,
    message: messagePart(),
    ...extra,
  };
}

export function engineAlert(
  id: string,
  severity: Alert["severity"] = "warning",
  extra: Partial<Alert> = {},
): Alert {
  return {
    alert_id: id,
    code: "endpoint_down",
    severity,
    state: "active",
    channel_id: null,
    endpoint_id: IDS.client,
    destination_id: null,
    summary: `summary ${id}`,
    next_action: "Check the cable",
    first_seen_at: "2026-11-06T11:00:00.000Z",
    last_seen_at: "2026-11-06T11:30:00.000Z",
    cleared_at: null,
    occurrences: 3,
    details: { count: 2, peer: "10.0.0.9", nested: { skipped: true } },
    ...extra,
  };
}

export function engineSnapshot(version = 7): EngineSnapshot {
  return {
    node: {
      node_id: "node-1",
      display_name: "Lab 1",
      version: "0.1.0",
      hosting_mode: "service",
      started_at: "2026-11-06T10:00:00.000Z",
      captured_at: "2026-11-06T12:00:10.000Z",
      uptime_seconds: 7210,
      node_state: "healthy",
      config: { active_version: version, pending_version: null, last_apply: null },
      channel_counts: { starting: 0, running: 2, retrying: 0, degraded: 0, stopped: 0 },
      endpoint_counts: {
        disabled: 1,
        connecting: 0,
        listening: 0,
        connected: 4,
        retrying: 1,
        disconnected: 0,
        error: 0,
      },
      totals: {
        bytes_in: 600,
        bytes_out: 790,
        messages_stored: 120,
        deliveries_unresolved: 6,
        deliveries_queued: 201,
        alerts_active: { warning: 1, critical: 0 },
      },
      store: {
        state: "ok",
        size_bytes: 4096,
        free_disk_bytes: 1_000_000,
        total_disk_bytes: 8_000_000,
      },
    },
    channels: [bridgeSnapshot(), messageChannelSnapshot()],
    endpoints: [
      endpointSnapshot(IDS.serial, { channels: [IDS.bridge] }),
      endpointSnapshot(IDS.listener, { channels: [IDS.message] }),
      endpointSnapshot(IDS.client, { channels: [IDS.bridge, IDS.message] }),
      endpointSnapshot(IDS.folder, { enabled: false, state: "disabled" }),
      endpointSnapshot(IDS.sftp, { state: "retrying" }),
      endpointSnapshot(IDS.folderOut, { channels: [IDS.message] }),
    ],
    alerts: [],
  };
}
