// How a transmission report words what each delivery proved (docs/spec/operator-console.md §2
// and §6). An acknowledgement below `accepted` never reads as delivered or successful.
import type { AckLevel, DeliveryState } from "@/api/types";
import { deliveryStatus, type Status } from "@/lib/status";

/** The level in the few words a grid cell has. */
const ACK_LEVEL_LABELS: Record<AckLevel, string> = {
  none: "None",
  handed_off: "Written, not confirmed",
  received: "Received",
  accepted: "Accepted",
};

export const ackLevelLabel = (level: AckLevel): string => ACK_LEVEL_LABELS[level];

/** What the level proved, in a sentence: the title of the cell. */
const ACK_LEVEL_SENTENCES: Record<AckLevel, string> = {
  none: "No acknowledgement was received.",
  handed_off: "Written to the destination. Not confirmed by it.",
  received: "Received by the destination. Processing not confirmed.",
  accepted: "Accepted by the destination's application.",
};

export const ackLevelSentence = (level: AckLevel): string => ACK_LEVEL_SENTENCES[level];

const STATE_LABELS: Record<DeliveryState, string> = {
  queued: "Queued",
  sending: "Sending",
  acknowledged: "Acknowledged",
  rejected: "Rejected by destination",
  timed_out: "No acknowledgement in time, may have been delivered",
  outcome_unknown: "Outcome unknown, may have been delivered",
  failed: "Failed, not delivered",
  cancelled: "Closed",
};

/** The delivery's state with the spec's label. */
export function stateStatus(state: DeliveryState): Status {
  return { tone: deliveryStatus(state).tone, label: STATE_LABELS[state] };
}
