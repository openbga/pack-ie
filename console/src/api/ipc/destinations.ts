// A message channel's destination (generated `DestinationSnapshot`) as the console's status.
import type {
  DestinationSnapshot,
  DestinationCounters as RawDestinationCounters,
} from "@/ipc/generated";
import type { DestinationCounters, DestinationStatus } from "../types";
import { lastErrorOf, refOf } from "./snapshot";

function countersFrom({
  attempts,
  acknowledged,
  rejected,
  timed_out,
  outcome_unknown,
  failed,
  bytes_out,
  ...unmapped
}: RawDestinationCounters): DestinationCounters {
  // A field the engine adds is a type error here, not a silently dropped figure.
  unmapped satisfies Record<string, never>;
  return {
    attempts,
    acknowledged,
    rejected,
    timedOut: timed_out,
    outcomeUnknown: outcome_unknown,
    failed,
    bytesOut: bytes_out,
  };
}

export function destinationFrom({
  destination_id,
  label,
  enabled,
  endpoint,
  protocol,
  queue,
  unresolved,
  counters,
  last_attempt_at,
  last_ack_at,
  last_error,
  ...unmapped
}: DestinationSnapshot): DestinationStatus {
  unmapped satisfies Record<string, never>;
  const { queued, sending, oldest_queued_at, next_attempt_at, ...unmappedQueue } = queue;
  unmappedQueue satisfies Record<string, never>;
  const { rejected, failed, timed_out, outcome_unknown, transform_errors, ...unmappedUnresolved } =
    unresolved;
  unmappedUnresolved satisfies Record<string, never>;
  return {
    destinationId: destination_id,
    label,
    endpoint: refOf(endpoint),
    protocol,
    queued,
    sending,
    oldestQueuedAt: oldest_queued_at,
    unresolved: { rejected, failed, timed_out, outcome_unknown },
    transformErrors: transform_errors,
    counters: countersFrom(counters),
    lastAcknowledgedAt: last_ack_at,
    lastAttemptAt: last_attempt_at,
    nextAttemptAt: next_attempt_at,
    lastError: lastErrorOf(last_error),
    enabled,
  };
}
