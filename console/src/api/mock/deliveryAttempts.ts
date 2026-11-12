import type { AckLevel, DeliveryAttempt } from "../types";
import type { DeliveryContext } from "./deliveryContext";
import type { DestinationInfo } from "./messageProfiles";
import { pickBy } from "./prng";
import { iso } from "./util";

/**
 * The highest level a destination can prove (spec §8): a file or a raw frame only `handed_off`;
 * ASTM and AU class B `received`, AU class A (no acknowledgement) `handed_off`; HL7
 * `accepted` once its application answers `AA`.
 */
export function topAckLevel(destination: DestinationInfo): AckLevel {
  switch (destination.protocol) {
    case "delimited_file":
      return "handed_off";
    case "astm":
      return "received";
    case "au":
      return destination.auClass === "a" ? "handed_off" : "received";
    case "hl7_mllp":
      return destination.ackMode === "none" ? "handed_off" : "accepted";
    default:
      return "handed_off";
  }
}

/** The acknowledgement code a destination answers with when it accepts, if its protocol has one. */
function acceptCode(destination: DestinationInfo): string | null {
  if (destination.protocol === "hl7_mllp") return destination.ackMode === "none" ? null : "AA";
  return destination.protocol === "delimited_file" ? null : "ACK";
}

export type Outcome = "failed" | "rejected" | "timed_out" | "outcome_unknown";

function errorCodeFor(destination: DestinationInfo): string {
  switch (destination.endpointKind) {
    case "serial":
      return "port_not_available";
    case "folder":
      return "access_denied";
    case "sftp":
      return "connection_timed_out";
    default:
      return "connection_refused";
  }
}

/**
 * The acknowledgement's own control id, which only an HL7 destination's reply has (MSH-10 of
 * the ACK); stable for an attempt, so a journey can read it twice.
 */
function ackControlIdOf(
  destination: DestinationInfo,
  ackCode: string | null,
  startMs: number,
): string | null {
  return ackCode !== null && destination.protocol === "hl7_mllp"
    ? `ACK${startMs.toString(36).toUpperCase()}`
    : null;
}

export function attempt(
  attemptNo: number,
  startMs: number,
  spec: Pick<DeliveryAttempt, "result" | "ackCode" | "errorCode" | "ackLevel"> & {
    durationMs: number;
    ackControlId?: string | null;
  },
): DeliveryAttempt {
  const { durationMs, ackControlId = null, ...rest } = spec;
  const ended = spec.result === null ? null : iso(startMs + durationMs);
  return { attemptNo, startedAt: iso(startMs), endedAt: ended, ackControlId, ...rest };
}

type Ending = (
  ctx: DeliveryContext,
  destination: DestinationInfo,
  no: number,
  startMs: number,
  h: number,
) => DeliveryAttempt;

const NO_ACK = { ackCode: null, ackLevel: "none" as AckLevel };

const ENDINGS: Record<Outcome, Ending> = {
  rejected: (_ctx, destination, no, startMs, h) => {
    const ackCode = destination.ackMode === "enhanced" ? "CE" : pickBy(["AE", "AR"], h);
    return attempt(no, startMs, {
      result: "rejected",
      errorCode: null,
      ackCode,
      ackControlId: ackControlIdOf(destination, ackCode, startMs),
      ackLevel: "none",
      durationMs: 140,
    });
  },
  timed_out: (ctx, destination, no, startMs, h) => {
    // An enhanced acknowledgement can reach `received` before the application never answers.
    const partial = destination.ackMode === "enhanced" && h % 2 === 0;
    return attempt(no, startMs, {
      result: "ack_timeout",
      errorCode: "ack_timeout",
      ackCode: partial ? "CA" : null,
      ackControlId: ackControlIdOf(destination, partial ? "CA" : null, startMs),
      ackLevel: partial ? "received" : "none",
      durationMs: ctx.profile.ackTimeoutMs,
    });
  },
  outcome_unknown: (_ctx, _destination, no, startMs, h) => {
    const restarted = h % 3 === 0;
    return attempt(no, startMs, {
      ...NO_ACK,
      result: restarted ? "engine_stopped" : "connection_lost_after_send",
      errorCode: restarted ? null : "connection_reset",
      durationMs: restarted ? 4_000 : 900,
    });
  },
  failed: (_ctx, destination, no, startMs, h) => {
    const timeout = destination.endpointKind === "tcp_client" && h % 4 === 0;
    return attempt(no, startMs, {
      ...NO_ACK,
      result: timeout ? "send_timeout" : "write_failed",
      errorCode: timeout ? "send_timeout" : errorCodeFor(destination),
      durationMs: timeout ? 5_000 : 60,
    });
  },
};

/** An attempt that ended with `outcome`; the delivery's state follows from its last one. */
export function attemptEnding(
  outcome: Outcome,
  ctx: DeliveryContext,
  destination: DestinationInfo,
  no: number,
  startMs: number,
  h: number,
): DeliveryAttempt {
  return ENDINGS[outcome](ctx, destination, no, startMs, h);
}

export function acknowledgedAttempt(destination: DestinationInfo, no: number, startMs: number) {
  return attempt(no, startMs, {
    result: "acknowledged",
    errorCode: null,
    ackCode: acceptCode(destination),
    ackControlId: ackControlIdOf(destination, acceptCode(destination), startMs),
    ackLevel: topAckLevel(destination),
    durationMs: 80,
  });
}
