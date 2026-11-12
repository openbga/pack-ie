import type {
  Delivery,
  DeliveryAttempt,
  DeliveryChip,
  DeliveryState,
  DeliverySummary,
} from "../types";
import {
  acknowledgedAttempt,
  attempt,
  attemptEnding,
  type Outcome,
  topAckLevel,
} from "./deliveryAttempts";
import type { DeliveryContext } from "./deliveryContext";
import type { DestinationInfo } from "./messageProfiles";
import { hash32, pickBy } from "./prng";
import { iso } from "./util";

/** Delivery states that need an operator (spec §4: `delivery_needs_attention`). */
const NEEDS_ATTENTION: ReadonlySet<DeliveryState> = new Set([
  "failed",
  "rejected",
  "timed_out",
  "outcome_unknown",
]);

/** Delivery states `close_delivery` accepts; `sending` cannot be closed. */
export const CLOSABLE: ReadonlySet<DeliveryState> = new Set([
  "queued",
  "rejected",
  "failed",
  "timed_out",
  "outcome_unknown",
]);

/** Delivery states whose message the destination may already have processed. */
export const MAY_BE_PROCESSED: ReadonlySet<DeliveryState> = new Set([
  "timed_out",
  "outcome_unknown",
  "acknowledged",
]);

const FINAL: ReadonlySet<DeliveryState> = new Set([
  "acknowledged",
  "rejected",
  "timed_out",
  "outcome_unknown",
  "failed",
  "cancelled",
]);

/** A delivery the engine is no longer working on (`queued` and `sending` are not final). */
export const isFinal = (state: DeliveryState) => FINAL.has(state);

const CLOSE_REASONS = [
  "Result confirmed by phone with the LIS administrator",
  "Order was cancelled by the ward",
  "Instrument rerun; this result was superseded",
  "Duplicate of a result already in the LIS",
];

/** `delivery_summary` of a message (spec §3). */
export function summarize(states: readonly DeliveryState[]): DeliverySummary {
  if (states.length === 0) return "none";
  if (states.some((state) => NEEDS_ATTENTION.has(state))) return "attention";
  if (states.some((state) => state === "queued" || state === "sending")) return "pending";
  return "complete";
}

export function chipOf(delivery: Delivery): DeliveryChip {
  return {
    deliveryId: delivery.id,
    destinationId: delivery.destinationId,
    destinationLabel: delivery.destinationLabel,
    state: delivery.state,
    ackLevel: delivery.ackLevel,
  };
}

/** The ways a delivery can end badly at a destination of this protocol (spec §8). */
function outcomesFor(destination: DestinationInfo): Outcome[] {
  switch (destination.protocol) {
    case "hl7_mllp":
      return ["failed", "rejected", "timed_out", "outcome_unknown"];
    case "astm":
    case "au":
      return ["failed", "outcome_unknown"];
    default:
      return ["failed"];
  }
}

function failingIndex(ctx: DeliveryContext): number {
  const count = Math.max(ctx.profile.destinations.length, 1);
  return hash32(ctx.seed, ctx.index, 500) % count;
}

function failingOutcome(ctx: DeliveryContext, destination: DestinationInfo, k: number): Outcome {
  return pickBy(outcomesFor(destination), hash32(ctx.seed, ctx.index, 400 + k));
}

/** A failed delivery whose mapping could not run: no body, no attempt. */
function isTransformError(ctx: DeliveryContext, destination: DestinationInfo, k: number): boolean {
  return (
    ctx.plan === "attention" &&
    destination.hasMappings &&
    failingOutcome(ctx, destination, k) === "failed" &&
    hash32(ctx.seed, ctx.index, 600 + k) % 3 === 0
  );
}

/** The states of a message's deliveries, in destination order; cheap, for filtering. */
export function deliveryStates(ctx: DeliveryContext): DeliveryState[] {
  const failing = failingIndex(ctx);
  return ctx.profile.destinations.map((destination, k) => {
    if (ctx.plan === "pending") return k === 0 ? "sending" : "queued";
    if (ctx.plan === "ok" || k !== failing) return "acknowledged";
    return ctx.plan === "closed" ? "cancelled" : failingOutcome(ctx, destination, k);
  });
}

type QueuedFields = Pick<
  Delivery,
  | "id"
  | "messageId"
  | "channelId"
  | "destinationId"
  | "destinationLabel"
  | "createdAt"
  | "nextAttemptAt"
  | "resendReason"
  | "mappingVersion"
  | "transformedHash"
>;

/** A delivery that has not been attempted yet. */
function queuedDelivery(fields: QueuedFields): Delivery {
  return {
    ...fields,
    state: "queued",
    ackLevel: "none",
    attempts: 0,
    totalAttempts: 0,
    updatedAt: fields.createdAt,
    reasonCode: null,
    closedReason: null,
    lastError: null,
    attemptLog: [],
  };
}

function emptyDelivery(
  ctx: DeliveryContext,
  destination: DestinationInfo,
  k: number,
  h: number,
): Delivery {
  return queuedDelivery({
    id: `${ctx.idPrefix}-${k + 1}`,
    messageId: `msg-${ctx.index}`,
    channelId: ctx.profile.channelId,
    destinationId: destination.destinationId,
    destinationLabel: destination.label,
    createdAt: iso(ctx.receivedAtMs + 15 + (h % 20)),
    nextAttemptAt: null,
    resendReason: null,
    mappingVersion: ctx.configVersion,
    transformedHash: hexHash(ctx, k),
  });
}

function hexHash(ctx: DeliveryContext, k: number): string {
  return Array.from({ length: 8 }, (_, part) =>
    hash32(ctx.seed, ctx.index, 700 + k * 8 + part)
      .toString(16)
      .padStart(8, "0"),
  ).join("");
}

/** Records `log` on `delivery` and moves its timestamps to the last attempt. */
function withAttempts(delivery: Delivery, log: DeliveryAttempt[]): Delivery {
  const last = log[log.length - 1];
  delivery.attemptLog = log;
  delivery.attempts = log.length;
  delivery.totalAttempts = log.length;
  delivery.updatedAt = last?.endedAt ?? last?.startedAt ?? delivery.updatedAt;
  return delivery;
}

function acknowledged(ctx: DeliveryContext, destination: DestinationInfo, k: number): Delivery {
  const h = hash32(ctx.seed, ctx.index, 300 + k);
  const delivery = emptyDelivery(ctx, destination, k, h);
  const log: DeliveryAttempt[] = [];
  let at = ctx.receivedAtMs + 120 + (h % 800);
  if (h % 10 === 0 && destination.protocol !== "delimited_file") {
    log.push(attemptEnding("failed", ctx, destination, 1, at, h));
    at += ctx.profile.initialMs;
  }
  log.push(acknowledgedAttempt(destination, log.length + 1, at));
  delivery.state = "acknowledged";
  delivery.ackLevel = topAckLevel(destination);
  return withAttempts(delivery, log);
}

/** A delivery that ended badly; retried failures run out of attempts. */
function failing(ctx: DeliveryContext, destination: DestinationInfo, k: number): Delivery {
  const h = hash32(ctx.seed, ctx.index, 400 + k);
  const delivery = emptyDelivery(ctx, destination, k, h);
  if (isTransformError(ctx, destination, k)) {
    delivery.state = "failed";
    delivery.reasonCode = "transform_error";
    delivery.lastError = "mapping rule copy to col.sample_id failed: OBR.3 is empty";
    delivery.mappingVersion = null;
    delivery.transformedHash = null;
    return delivery;
  }
  const outcome = failingOutcome(ctx, destination, k);
  const tries = outcome === "failed" ? Math.min(ctx.profile.maxAttempts, 5) : 1;
  const log: DeliveryAttempt[] = [];
  let at = ctx.receivedAtMs + 200 + (h % 600);
  for (let n = 1; n <= tries; n++) {
    log.push(attemptEnding(outcome, ctx, destination, n, at, h));
    at += ctx.profile.initialMs * 2 ** n;
  }
  delivery.state = outcome;
  delivery.ackLevel = log[log.length - 1]?.ackLevel ?? "none";
  if (outcome === "outcome_unknown" && log[0]?.result === "engine_stopped") {
    delivery.reasonCode = "engine_restarted";
  }
  const last = log[log.length - 1];
  if (last !== undefined) {
    delivery.lastError =
      last.ackCode !== null && last.errorCode === null
        ? `${destination.label} answered ${last.ackCode}`
        : `${destination.label}: attempt ${last.attemptNo} ended with ${last.errorCode ?? "no answer"}`;
  }
  return withAttempts(delivery, log);
}

/** `queued` behind the delivery in flight, sometimes waiting for a retry after a failed attempt. */
function waiting(ctx: DeliveryContext, destination: DestinationInfo, k: number): Delivery {
  const h = hash32(ctx.seed, ctx.index, 800 + k);
  const delivery = emptyDelivery(ctx, destination, k, h);
  if (k === 0) {
    delivery.state = "sending";
    return withAttempts(delivery, [
      attempt(1, ctx.receivedAtMs + 300, {
        result: null,
        ackCode: null,
        errorCode: null,
        ackLevel: "none",
        durationMs: 0,
      }),
    ]);
  }
  if (h % 3 !== 0 || destination.protocol === "delimited_file") return delivery;
  const failed = attemptEnding("failed", ctx, destination, 1, ctx.receivedAtMs + 250, h);
  withAttempts(delivery, [failed]);
  delivery.nextAttemptAt = iso(ctx.receivedAtMs + 250 + ctx.profile.initialMs * 2);
  return delivery;
}

/** One delivery per destination, consistent with how the message's deliveries stand. */
export function buildDeliveries(ctx: DeliveryContext): Delivery[] {
  const failing_ = failingIndex(ctx);
  const closedReason = pickBy(CLOSE_REASONS, hash32(ctx.seed, ctx.index, 501));
  const removed = hash32(ctx.seed, ctx.index, 502) % 5 === 0;
  return ctx.profile.destinations.map((destination, k) => {
    if (ctx.plan === "pending") return waiting(ctx, destination, k);
    if (ctx.plan === "ok" || k !== failing_) return acknowledged(ctx, destination, k);
    const delivery = failing(ctx, destination, k);
    if (ctx.plan === "closed") {
      delivery.state = "cancelled";
      if (removed) delivery.reasonCode = "destination_removed";
      else delivery.closedReason = closedReason;
    }
    return delivery;
  });
}

// ── Operator commands on a delivery ──────────────────────────────────────────────────────

/** `same_message` resend: a new attempt cycle, attempts back to zero, the total keeps counting. */
export function requeue(delivery: Delivery, nowMs: number, reason: string | null = null): void {
  delivery.state = "queued";
  delivery.ackLevel = "none";
  delivery.attempts = 0;
  delivery.nextAttemptAt = iso(nowMs);
  delivery.reasonCode = null;
  delivery.closedReason = null;
  delivery.resendReason = reason;
  delivery.updatedAt = iso(nowMs);
}

/** The destination accepts the delivery that was queued again. */
export function settle(
  delivery: Delivery,
  destination: DestinationInfo,
  nowMs: number,
  configVersion: number,
): void {
  const no = delivery.totalAttempts + 1;
  delivery.attemptLog.push(acknowledgedAttempt(destination, no, nowMs));
  delivery.state = "acknowledged";
  delivery.ackLevel = topAckLevel(destination);
  delivery.attempts += 1;
  delivery.totalAttempts = no;
  delivery.nextAttemptAt = null;
  delivery.updatedAt = iso(nowMs + 80);
  if (delivery.transformedHash === null) {
    // The current mapping ran on the stored original and its output replaced the empty body.
    delivery.mappingVersion = configVersion;
    delivery.transformedHash = hash32(configVersion, no, 900)
      .toString(16)
      .padStart(8, "0")
      .repeat(8);
  }
}

export function cancel(delivery: Delivery, reason: string, nowMs: number): void {
  delivery.state = "cancelled";
  delivery.closedReason = reason;
  delivery.nextAttemptAt = null;
  delivery.updatedAt = iso(nowMs);
}

/** A delivery that the pipeline just created for a resent message. */
export function createdDelivery(
  messageId: string,
  idPrefix: string,
  channelId: string,
  destination: DestinationInfo,
  k: number,
  nowMs: number,
  configVersion: number,
  resendReason: string | null = null,
): Delivery {
  const at = iso(nowMs);
  return queuedDelivery({
    id: `${idPrefix}-${k + 1}`,
    messageId,
    channelId,
    destinationId: destination.destinationId,
    destinationLabel: destination.label,
    createdAt: at,
    nextAttemptAt: at,
    resendReason,
    mappingVersion: configVersion,
    transformedHash: hash32(configVersion, k, 901).toString(16).padStart(8, "0").repeat(8),
  });
}
