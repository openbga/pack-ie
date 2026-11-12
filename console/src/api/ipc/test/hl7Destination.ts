import type { DestinationStatus } from "../../types";
import { IDS } from "./configFixtures";

/** The console's reading of the rich message channel's HL7 destination (`messagePart()`). */
export const HL7_DESTINATION_STATUS: DestinationStatus = {
  destinationId: IDS.hl7,
  label: "LIS (HL7)",
  endpoint: { endpointId: IDS.client, label: "LIS HL7", state: "connected" },
  protocol: "hl7_mllp",
  queued: 200,
  sending: 1,
  oldestQueuedAt: "2026-11-06T11:30:00.000Z",
  unresolved: { rejected: 2, failed: 0, timed_out: 0, outcome_unknown: 0 },
  transformErrors: 0,
  counters: {
    attempts: 9,
    acknowledged: 7,
    rejected: 2,
    timedOut: 0,
    outcomeUnknown: 0,
    failed: 0,
    bytesOut: 700,
  },
  lastAcknowledgedAt: "2026-11-06T12:00:02.000Z",
  lastAttemptAt: "2026-11-06T12:00:04.000Z",
  nextAttemptAt: "2026-11-06T12:00:30.000Z",
  lastError: {
    at: "2026-11-06T12:00:04.000Z",
    summary: "Channel ch-message destination d-hl7 send failed.",
    nextAction: "Retrying in 5s.",
  },
  enabled: true,
};
