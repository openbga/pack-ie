import type {
  DestinationConfig,
  DestinationStatus,
  EndpointRef,
  MessageChannelConfig,
} from "../types";
import { hashString } from "./prng";
import type { EndpointRuntime } from "./runtimeState";
import { toLastError } from "./runtimeState";
import type { SnapshotInputs } from "./snapshot";
import { HOUR, iso, MINUTE } from "./util";

function isFaulty(ref: EndpointRef): boolean {
  return ref.state === "error" || ref.state === "retrying" || ref.state === "disconnected";
}

interface Share {
  /** Queued and in flight together; the channel's queue depth is the sum over destinations. */
  waiting: number;
  unresolved: DestinationStatus["unresolved"];
  /** Of the failed ones, those whose mapping failed. */
  transformErrors: number;
  acknowledged: number;
}

/**
 * Delivery counts for one destination: pending and failed deliveries land on the destination
 * that is failing (or the last one), so the totals match the channel's queue.
 */
function share(
  config: MessageChannelConfig,
  inputs: SnapshotInputs,
  index: number,
  badIndex: number,
): Share {
  const rt = inputs.runtime.channels.get(config.id);
  const none = { rejected: 0, failed: 0, timed_out: 0, outcome_unknown: 0 };
  if (rt === undefined)
    return { waiting: 0, unresolved: none, transformErrors: 0, acknowledged: 0 };
  const destinations = config.destinations.length;
  const others = Math.max(destinations - 1, 1);
  const acknowledged = Math.floor((rt.retainedAcked + rt.counters.messagesOut) / destinations);
  let waiting: number;
  if (destinations === 1) waiting = rt.queueDepth;
  else if (index === badIndex) waiting = Math.ceil(rt.queueDepth * 0.8);
  else waiting = Math.floor((rt.queueDepth * 0.2) / others);
  if (index !== badIndex) return { waiting, unresolved: none, transformErrors: 0, acknowledged };
  const attention = rt.needsAttention;
  const rejected = Math.floor(attention * 0.25);
  const timedOut = Math.floor(attention * 0.25);
  const unknown = Math.floor(attention * 0.1);
  const failed = attention - rejected - timedOut - unknown;
  return {
    waiting,
    // The destination with failures has acknowledged fewer than the healthy ones.
    acknowledged: destinations > 1 && attention > 0 ? Math.floor(acknowledged / 2) : acknowledged,
    // A third of the failures never reached the destination: their mapping failed.
    transformErrors: Math.floor(failed / 3),
    unresolved: { rejected, timed_out: timedOut, outcome_unknown: unknown, failed },
  };
}

const ATTEMPT_PERIOD_MS = 5000;

/**
 * Earliest scheduled attempt: the endpoint's retry while it fails, otherwise the next tick of
 * the destination's own 5 s cycle. Null when nothing waits or nothing is being sent.
 */
function nextAttemptAt(
  destination: DestinationConfig,
  queued: number,
  stopped: boolean,
  endpoint: EndpointRuntime | undefined,
  nowMs: number,
): string | null {
  if (!destination.enabled || stopped || queued === 0) return null;
  if (endpoint?.fault != null && endpoint.nextRetryMs !== null) return iso(endpoint.nextRetryMs);
  const offset = hashString(destination.id) % 1000;
  return iso((Math.floor(nowMs / ATTEMPT_PERIOD_MS) + 1) * ATTEMPT_PERIOD_MS + offset);
}

export function destinationStatuses(
  config: MessageChannelConfig,
  inputs: SnapshotInputs,
  refs: EndpointRef[],
): DestinationStatus[] {
  const faulty = config.destinations.findIndex((_, i) => {
    const ref = refs[i + 1];
    return ref !== undefined && isFaulty(ref);
  });
  const badIndex = faulty >= 0 ? faulty : config.destinations.length - 1;
  const rt = inputs.runtime.channels.get(config.id);
  const stopped = inputs.derived.channels.get(config.id)?.state === "stopped";
  return config.destinations.map((destination, index) => {
    const endpoint = refs[index + 1] as EndpointRef;
    const endpointRuntime = inputs.runtime.endpoints.get(destination.endpointId);
    const healthy = !isFaulty(endpoint);
    const lastOk = healthy
      ? (rt?.lastActivityMs ?? inputs.nowMs - MINUTE)
      : (endpointRuntime?.stateSince ?? inputs.nowMs) - 30_000;
    const { waiting, unresolved, transformErrors, acknowledged } = share(
      config,
      inputs,
      index,
      badIndex,
    );
    const active = destination.enabled && !stopped && healthy && waiting > 0;
    const sending = active ? 1 : 0;
    const queued = waiting - sending;
    const attention = Object.values(unresolved).reduce((sum, count) => sum + count, 0);
    return {
      destinationId: destination.id,
      label: destination.label,
      endpoint,
      protocol: destination.protocol,
      queued,
      sending,
      oldestQueuedAt:
        waiting > 0 ? iso(inputs.nowMs - Math.min(waiting * 4000 + 2000, 6 * HOUR)) : null,
      unresolved,
      transformErrors,
      counters: {
        attempts: acknowledged + attention + sending,
        acknowledged,
        rejected: unresolved.rejected,
        timedOut: unresolved.timed_out,
        outcomeUnknown: unresolved.outcome_unknown,
        failed: unresolved.failed,
        bytesOut: acknowledged * 640,
      },
      lastAcknowledgedAt: iso(lastOk),
      lastAttemptAt: iso(healthy ? lastOk : (endpointRuntime?.lastAttemptMs ?? lastOk)),
      nextAttemptAt: nextAttemptAt(destination, queued, stopped, endpointRuntime, inputs.nowMs),
      lastError: toLastError(endpointRuntime?.fault?.error ?? null),
      enabled: destination.enabled,
    };
  });
}
