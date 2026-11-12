import type {
  DeliveryState,
  MessageDetail,
  MessageState,
  MessageSummary,
  ResendMode,
} from "@/api/types";
import {
  isInProgress,
  mayHaveBeenProcessed,
  needsAttention,
  newMessageOnly,
} from "./messageWording";

/** What the resend dialog needs to know about a message. */
export interface ResendSubject {
  messageId: string;
  /** What people call the message: its control id, else its sample id, else its id. */
  identifier: string;
  channelLabel: string;
  state: MessageState;
  /** One entry per delivery; none for a message nothing was queued for. */
  destinations: { id: string; label: string; state: DeliveryState }[];
}

const identifierOf = (summary: MessageSummary) =>
  summary.controlId ?? summary.sampleId ?? summary.id;

/** The dialog's subject from a grid row, whose chips carry each delivery's state. */
export function subjectOfSummary(summary: MessageSummary): ResendSubject {
  return {
    messageId: summary.id,
    identifier: identifierOf(summary),
    channelLabel: summary.channelLabel,
    state: summary.state,
    destinations: summary.deliveries.map((chip) => ({
      id: chip.destinationId,
      label: chip.destinationLabel,
      state: chip.state,
    })),
  };
}

/** The dialog's subject from the record. */
export function subjectOfDetail(detail: MessageDetail): ResendSubject {
  return {
    ...subjectOfSummary(detail.summary),
    destinations: detail.deliveries.map((delivery) => ({
      id: delivery.destinationId,
      label: delivery.destinationLabel,
      state: delivery.state,
    })),
  };
}

/** The modes the engine accepts for the message (message-channels.md §9). */
export function offeredModes(subject: ResendSubject): ResendMode[] {
  const onlyNew = newMessageOnly(subject.state) || subject.destinations.length === 0;
  return onlyNew ? ["new_message"] : ["same_message", "new_message"];
}

/** The destinations a resend starts with: those whose delivery needs attention. */
export function defaultDestinations(subject: ResendSubject): string[] {
  return subject.destinations.filter((d) => needsAttention(d.state)).map((d) => d.id);
}

/**
 * The selected destinations the mode cannot resend: a delivery still queued or sending can
 * only be answered with a new message (`delivery_in_progress`).
 */
export function blockedDestinations(
  subject: ResendSubject,
  selected: readonly string[],
  mode: ResendMode,
): string[] {
  if (mode === "new_message") return [];
  return subject.destinations
    .filter((d) => selected.includes(d.id) && isInProgress(d.state))
    .map((d) => d.id);
}

/**
 * Whether the resend needs the duplicate-risk confirmation: a selected delivery was
 * acknowledged, timed out or has an unknown outcome, so its destination may already have the
 * message (message-channels.md §9, both modes).
 */
export function needsRiskConfirmation(
  subject: ResendSubject,
  selected: readonly string[],
): boolean {
  return subject.destinations.some((d) => selected.includes(d.id) && mayHaveBeenProcessed(d.state));
}

/**
 * Whether the resend needs the superseded-message confirmation: a newer revision exists (the
 * record says so), or the engine refused the resend because one does, which the record read
 * earlier did not know (compliance P10).
 */
export function needsSupersededConfirmation(
  revisedBy: string | null,
  refusedAsSuperseded: boolean,
): boolean {
  return revisedBy !== null || refusedAsSuperseded;
}
