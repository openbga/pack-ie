/** Message records as the engine returns them, and an engine that serves the rich document. */

import type { DeliveryRecord, MessageRecord, MessageSummary } from "@/ipc/generated";
import type { MessageQuery, ResendRequest } from "../../types";
import { IpcEngineApi } from "../IpcEngineApi";
import { IDS, richConfig } from "./configFixtures";
import { FakeBridge } from "./fakeBridge";

export function summary(extra: Partial<MessageSummary> = {}): MessageSummary {
  return {
    message_id: "msg-1",
    channel_id: IDS.message,
    channel_label: "Chemistry results",
    seq: 1,
    direction: "inbound",
    state: "routed",
    origin: "received",
    received_at: "2026-11-06T12:00:00Z",
    protocol: "astm",
    message_type: "astm",
    control_id: "C-1",
    sample_id: "S-1",
    patient_id: "P-1",
    byte_length: 120,
    source_peer: "10.0.0.5:4000",
    config_version: 7,
    resent_from: null,
    delivery_summary: "complete",
    deliveries: [
      {
        delivery_id: "d-1",
        destination_id: IDS.hl7,
        destination_label: "LIS (HL7)",
        state: "acknowledged",
        ack_level: "accepted",
      },
    ],
    ...extra,
  };
}

export function delivery(extra: Partial<DeliveryRecord> = {}): DeliveryRecord {
  return {
    delivery_id: "d-1",
    message_id: "msg-1",
    channel_id: IDS.message,
    destination_id: IDS.hl7,
    destination_label: "LIS (HL7)",
    state: "acknowledged",
    ack_level: "accepted",
    attempts: 2,
    total_attempts: 2,
    created_at: "2026-11-06T12:00:01Z",
    updated_at: "2026-11-06T12:00:09Z",
    next_attempt_at: null,
    reason_code: null,
    last_error: null,
    closed_reason: null,
    resend_reason: null,
    mapping_version: 7,
    transformed_hash: "abc",
    attempt_log: [
      {
        attempt_no: 1,
        started_at: "2026-11-06T12:00:02Z",
        ended_at: "2026-11-06T12:00:04Z",
        result: "ack_timeout",
        ack_code: null,
        ack_control_id: null,
        error_code: "ack_timeout",
        ack_level: "none",
      },
      {
        attempt_no: 2,
        started_at: "2026-11-06T12:00:06Z",
        ended_at: "2026-11-06T12:00:08Z",
        result: "acknowledged",
        ack_code: "AA",
        ack_control_id: "ACK-77",
        error_code: null,
        ack_level: "accepted",
      },
    ],
    ...extra,
  };
}

export function record(extra: Partial<MessageRecord> = {}): MessageRecord {
  return {
    summary: summary(),
    committed_at: "2026-11-06T12:00:00Z",
    content_hash: "hash",
    source_identity: null,
    source_endpoint_id: IDS.listener,
    source_file: null,
    response_code: "ACK",
    responded_at: "2026-11-06T12:00:00Z",
    abort_reason: null,
    reason_code: null,
    duplicate_of: null,
    revision_of: null,
    revised_by: null,
    deliveries: [delivery()],
    ...extra,
  };
}

export function engine() {
  const bridge = new FakeBridge()
    .command("ipc_connection", () => ({
      state: "connected",
      endpoint: null,
      error: null,
      attempt: 0,
      engine: null,
      since: "",
    }))
    .request("get_config", () => ({ type: "config", document: richConfig() }));
  return { bridge, api: new IpcEngineApi(bridge) };
}

export const emptyFilter = {
  since: null,
  until: null,
  channels: [],
  directions: [],
  states: [],
  delivery_states: [],
  delivery_summaries: [],
  identifier: null,
  text: null,
  message_id_prefix: null,
};

export const base: MessageQuery = {
  filter: {},
  sort: { field: "receivedAt", descending: true },
  offset: 0,
  limit: 100,
};

/** A resend of the first message to all its destinations, as the operator asked for no more. */
export const RESEND_ALL: ResendRequest = {
  messageId: "msg-1",
  destinations: "all",
  mode: "same_message",
  confirmDuplicateRisk: false,
};
