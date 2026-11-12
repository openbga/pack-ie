// How the store's states, levels and reason codes are worded on the Messages screens
// (docs/spec/operator-console.md §2 and §6, message-channels.md §3 to §9). One place, so a
// state reads the same in the grid, the record and the dialogs.
import type {
  AckLevel,
  AttemptResult,
  Delivery,
  DeliveryState,
  MessageDirection,
  MessageOrigin,
  MessageState,
  RejectionCode,
} from "@/api/types";
import type { Status } from "@/lib/status";
import { deliveryStatus } from "@/lib/status";

export const MESSAGE_STATES: readonly MessageState[] = [
  "receiving",
  "received",
  "routed",
  "filtered",
  "duplicate",
  "invalid",
  "aborted",
  "observed",
];

export const DELIVERY_STATES: readonly DeliveryState[] = [
  "queued",
  "sending",
  "acknowledged",
  "rejected",
  "timed_out",
  "outcome_unknown",
  "failed",
  "cancelled",
];

export const MESSAGE_DIRECTIONS: readonly MessageDirection[] = ["inbound", "a_to_b", "b_to_a"];

// ── Message ──────────────────────────────────────────────────────────────────────────────

const MESSAGE_STATUS: Record<MessageState, Status> = {
  receiving: { tone: "info", label: "Receiving" },
  received: { tone: "info", label: "Received" },
  routed: { tone: "neutral", label: "Routed" },
  filtered: { tone: "neutral", label: "Filtered" },
  duplicate: { tone: "neutral", label: "Duplicate" },
  invalid: { tone: "danger", label: "Invalid" },
  aborted: { tone: "warning", label: "Aborted" },
  observed: { tone: "neutral", label: "Observed" },
};

export const messageStateStatus = (state: MessageState): Status => MESSAGE_STATUS[state];

/** What a message state means, in one sentence (message-channels.md §3). */
export const MESSAGE_STATE_MEANING: Record<MessageState, string> = {
  receiving: "A multi-frame transfer is in progress; frames are stored as they arrive.",
  received: "The complete message is stored; the channel has not finished processing it.",
  routed: "The channel finished processing it and created a delivery for each destination.",
  filtered: "A filter, or the lack of an enabled destination, excluded it. Nothing was sent.",
  duplicate: "It repeats an earlier message with the same identity and content. Nothing was sent.",
  invalid: "The bytes are complete but could not be parsed or validated. Nothing was sent.",
  aborted: "The transfer ended before a complete message arrived. Nothing was sent.",
  observed: "Recorded from a bridge's tap. It was never processed and nothing was sent.",
};

const DIRECTION_LABELS: Record<MessageDirection, string> = {
  inbound: "Inbound",
  a_to_b: "Side A to side B",
  b_to_a: "Side B to side A",
};

export const directionText = (direction: MessageDirection): string => DIRECTION_LABELS[direction];

/** The direction in the few characters a narrow column has; `directionText` is its title. */
const DIRECTION_SHORT: Record<MessageDirection, string> = {
  inbound: "Inbound",
  a_to_b: "A to B",
  b_to_a: "B to A",
};

export const directionShortText = (direction: MessageDirection): string =>
  DIRECTION_SHORT[direction];

const ORIGIN_LABELS: Record<MessageOrigin, string> = {
  received: "Received from the sender",
  resend: "Created by a resend",
  tap: "Observed by a bridge tap",
};

export const originText = (origin: MessageOrigin): string => ORIGIN_LABELS[origin];

/** A message can be resent when its channel processed it, or tried to; others never were. */
export function canResend(state: MessageState): boolean {
  return state !== "receiving" && state !== "received" && state !== "observed";
}

/** The states whose messages can only be resent as a new message (message-channels.md §9). */
export function newMessageOnly(state: MessageState): boolean {
  return (
    state === "invalid" || state === "aborted" || state === "filtered" || state === "duplicate"
  );
}

// ── Delivery ─────────────────────────────────────────────────────────────────────────────

/** The short word a chip and a badge show. */
const DELIVERY_CHIP_LABELS: Record<DeliveryState, string> = {
  queued: "Queued",
  sending: "Sending",
  acknowledged: "Acknowledged",
  rejected: "Rejected",
  timed_out: "No acknowledgement",
  outcome_unknown: "Outcome unknown",
  failed: "Failed",
  cancelled: "Closed",
};

/** The delivery's status as a chip shows it: the state's tone with the short label. */
export function deliveryChipStatus(state: DeliveryState): Status {
  return { tone: deliveryStatus(state).tone, label: DELIVERY_CHIP_LABELS[state] };
}

/** The labels of operator-console.md §2: what the state means for the message. */
const DELIVERY_LABELS: Record<DeliveryState, string> = {
  queued: "Queued",
  sending: "Sending",
  acknowledged: "Acknowledged",
  rejected: "Rejected by destination",
  timed_out: "No acknowledgement in time, may have been delivered",
  outcome_unknown: "Outcome unknown, may have been delivered",
  failed: "Failed, not delivered",
  cancelled: "Closed",
};

export const deliveryLabel = (state: DeliveryState): string => DELIVERY_LABELS[state];

/** The delivery's state with the label of the spec, for a record. */
export function deliveryRecordStatus(state: DeliveryState): Status {
  return { tone: deliveryStatus(state).tone, label: DELIVERY_LABELS[state] };
}

/** `ack_level` in words (operator-console.md §6); a transport acknowledgement is never success. */
const ACK_LEVEL_WORDS: Record<AckLevel, string> = {
  none: "No acknowledgement was received.",
  handed_off: "Written to the destination. Not confirmed by it.",
  received: "Received by the destination. Processing not confirmed.",
  accepted: "Accepted by the destination's application.",
};

export const ackLevelText = (level: AckLevel): string => ACK_LEVEL_WORDS[level];

/** A few words for a chip, which has little room. */
const ACK_LEVEL_SHORT: Record<AckLevel, string> = {
  none: "no confirmation",
  handed_off: "written",
  received: "received",
  accepted: "accepted",
};

/** The state as a chip's badge words it; an acknowledgement says what it proved. */
export function chipStateText(chip: { state: DeliveryState; ackLevel: AckLevel }): string {
  const state = DELIVERY_CHIP_LABELS[chip.state];
  return chip.state === "acknowledged" ? `${state} (${ACK_LEVEL_SHORT[chip.ackLevel]})` : state;
}

/**
 * The state as a chip in the grid words it, where room is short: an acknowledgement is named
 * by what it proved (the spec's words for the levels), so it still never reads as success of
 * processing. The destination's full wording is the chip's tooltip.
 */
const ACK_LEVEL_BADGE: Record<AckLevel, string> = {
  none: "Acknowledged",
  handed_off: "Written",
  received: "Received",
  accepted: "Accepted",
};

export function chipBadgeText(chip: { state: DeliveryState; ackLevel: AckLevel }): string {
  return chip.state === "acknowledged"
    ? ACK_LEVEL_BADGE[chip.ackLevel]
    : DELIVERY_CHIP_LABELS[chip.state];
}

/** A chip's text for assistive technology and tooltips: the destination and its state. */
export function chipText(chip: {
  destinationLabel: string;
  state: DeliveryState;
  ackLevel: AckLevel;
}): string {
  return `${chip.destinationLabel}: ${chipStateText(chip)}`;
}

/**
 * What a delivery proved, in a sentence. A delivery that is not acknowledged says which level
 * it did reach, so "may have been delivered" always comes with what was and was not received.
 */
export function ackSentence(delivery: Pick<Delivery, "state" | "ackLevel">): string {
  if (delivery.state === "queued" || delivery.state === "sending") {
    return delivery.ackLevel === "none"
      ? "Nothing has been confirmed by the destination yet."
      : ackLevelText(delivery.ackLevel);
  }
  return ackLevelText(delivery.ackLevel);
}

const ATTEMPT_RESULT_LABELS: Record<AttemptResult, string> = {
  acknowledged: "Acknowledged",
  rejected: "Rejected by the destination",
  write_failed: "Write failed",
  send_timeout: "Send timed out",
  ack_timeout: "No acknowledgement in time",
  connection_lost_after_send: "Connection lost after the message was sent",
  engine_stopped: "Engine stopped during the attempt",
  transform_error: "Mapping failed",
};

export const attemptResultText = (result: AttemptResult | null): string =>
  result === null ? "In progress" : ATTEMPT_RESULT_LABELS[result];

/** Deliveries a resend in `same_message` mode must wait for (`delivery_in_progress`). */
export const isInProgress = (state: DeliveryState): boolean =>
  state === "queued" || state === "sending";

/** Deliveries that may already have been processed: resending needs the duplicate-risk check. */
export function mayHaveBeenProcessed(state: DeliveryState): boolean {
  return state === "timed_out" || state === "outcome_unknown" || state === "acknowledged";
}

/** Deliveries whose earlier attempt may have reached the destination: closing settles nothing about it. */
export const isUncertain = (state: DeliveryState): boolean =>
  state === "timed_out" || state === "outcome_unknown";

/** What an acknowledgement level proved, as the end of "the earlier attempt may have reached it: …". */
const ACK_EVIDENCE: Record<AckLevel, string> = {
  none: "no acknowledgement was received",
  handed_off: "it was written to the destination, which did not confirm it",
  received: "the destination received it, but processing was not confirmed",
  accepted: "the destination's application accepted it",
};

/**
 * What the destination is known to have answered, for a delivery whose outcome is uncertain:
 * the last acknowledgement code any attempt received, else what the acknowledgement level
 * proved (which is "no acknowledgement was received" at level none).
 */
export function ackEvidence(delivery: Pick<Delivery, "ackLevel" | "attemptLog">): string {
  const code = delivery.attemptLog.findLast((attempt) => attempt.ackCode !== null)?.ackCode;
  return code ? `the last acknowledgement code was ${code}` : ACK_EVIDENCE[delivery.ackLevel];
}

/** Deliveries an operator still has to resolve (they stay listed until resent or closed). */
export function needsAttention(state: DeliveryState): boolean {
  return (
    state === "failed" ||
    state === "rejected" ||
    state === "timed_out" ||
    state === "outcome_unknown"
  );
}

/** Deliveries `close_delivery` accepts (a delivery in `sending` cannot be closed). */
export function canClose(state: DeliveryState): boolean {
  return state === "queued" || needsAttention(state);
}

// ── Reason codes ─────────────────────────────────────────────────────────────────────────

const REASON_WORDS: Record<string, string> = {
  // Filtered and invalid messages
  source_filter: "A filter of the channel excluded it.",
  destination_filters: "The filters of every destination excluded it.",
  no_destination: "No destination of the channel is enabled.",
  parse_error: "The bytes could not be parsed.",
  unparseable: "The bytes could not be parsed.",
  control_message:
    "The frame is an instrument request, not a result, so it was stored and not routed.",
  missing_required_field: "A required field of the message is missing.",
  // Aborted transfers
  retry_limit_exceeded: "The sender repeated a frame more often than allowed.",
  receive_timeout: "No data arrived within the receive timeout.",
  incomplete_record: "A record ended before it was complete.",
  empty_transfer: "The transfer ended without any record.",
  message_too_large: "The message exceeded the size limit.",
  connection_lost: "The connection was lost during the transfer.",
  transport_error: "A transport error ended the transfer.",
  channel_stopped: "The channel was stopped during the transfer.",
  file_too_large: "The file exceeded the size limit and was not read.",
  startup_interrupted: "The engine restarted during the transfer.",
  // Deliveries
  transform_error:
    "The destination's mapping could not be applied to this message, so nothing was sent.",
  engine_restarted: "The engine restarted during the attempt, so the outcome is unknown.",
  channel_paused: "The channel was paused during the attempt.",
  destination_removed:
    "The destination was removed from the configuration, so the delivery was closed.",
  channel_removed: "The channel was removed from the configuration, so the delivery was closed.",
};

/** A reason code in words; a code this console does not know is shown as it is. */
export function reasonText(code: string): string {
  return REASON_WORDS[code] ?? `Reason code ${code}.`;
}

/** The reason in words with its code, so a person can quote the code to support. */
export function reasonWithCode(code: string): string {
  const words = REASON_WORDS[code];
  return words === undefined ? `Reason code ${code}.` : `${words} (${code})`;
}

// ── Rejections of commands ───────────────────────────────────────────────────────────────

const REJECTION_TITLES: Partial<Record<RejectionCode, string>> = {
  not_found: "Not found",
  delivery_in_progress: "Delivery in progress",
  invalid_state: "Not possible in this state",
  duplicate_risk_unconfirmed: "Duplicate risk not confirmed",
  destination_not_found: "Destination not available",
  reason_required: "Reason required",
  superseded: "Newer revision exists",
};

export const rejectionTitle = (code: RejectionCode): string => REJECTION_TITLES[code] ?? code;
