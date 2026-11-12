// What an apply did to each channel, in one set of words (configuration.md section 10, Result):
// the apply and rollback results, the version history and the change report all use these.
import type { ApplyResult, ChannelOutcome, ChannelOutcomeKind, ConfigDocument } from "@/api/types";
import type { Tone } from "@/components/tones";

/** The words for each outcome, in the order they are listed: what needs attention first. */
const OUTCOME_WORDS: Record<ChannelOutcomeKind, string> = {
  failed: "failed",
  restarted: "restarted",
  updated: "updated",
  started: "started",
  stopped: "stopped",
  removed: "removed",
  unchanged: "unchanged",
};

const ORDER = Object.keys(OUTCOME_WORDS) as ChannelOutcomeKind[];

/** Status tones; the word always accompanies the tone, so color and shape never stand alone. */
export const OUTCOME_TONES: Record<ChannelOutcomeKind, Tone> = {
  failed: "danger",
  restarted: "warning",
  updated: "info",
  started: "success",
  stopped: "hold",
  removed: "hold",
  unchanged: "neutral",
};

/** `restarted` for the badge and the sentence start: the word with a capital. */
export const capitalized = (kind: ChannelOutcomeKind): string =>
  OUTCOME_WORDS[kind].charAt(0).toUpperCase() + OUTCOME_WORDS[kind].slice(1);

/** The outcomes that did something to a channel, failures first, then in the order above. */
export function sortedOutcomes(outcomes: readonly ChannelOutcome[]): ChannelOutcome[] {
  return [...outcomes].sort((a, b) => ORDER.indexOf(a.outcome) - ORDER.indexOf(b.outcome));
}

/**
 * A line for the history grid: "2 restarted, 1 updated, 1 failed" (failures last, so the line
 * ends on them). Channels that were left alone are not counted; no record (an old version, the startup import) says so.
 */
export function summarizeOutcomes(outcomes: readonly ChannelOutcome[]): string {
  if (outcomes.length === 0) return "No outcomes recorded";
  const parts = [
    ...ORDER.filter((kind) => kind !== "unchanged" && kind !== "failed"),
    "failed" as const,
  ]
    .map((kind) => [kind, outcomes.filter((each) => each.outcome === kind).length] as const)
    .filter(([, count]) => count > 0)
    .map(([kind, count]) => `${count} ${OUTCOME_WORDS[kind]}`);
  return parts.length > 0 ? parts.join(", ") : "No channel changed";
}

/** A channel's name for an id: its label in the first of `lists` that has it, else the id. */
export function channelNamer(
  ...lists: readonly (ConfigDocument["channels"] | undefined)[]
): (channelId: string) => string {
  return (channelId) => {
    for (const channels of lists) {
      const found = channels?.find((channel) => channel.id === channelId);
      if (found) return found.label;
    }
    return channelId;
  };
}

/** What the operator is told after an apply or rollback: tone, title and the lines under it. */
export interface ApplyReport {
  tone: "success" | "warning";
  title: string;
  description: string;
}

/**
 * The toast text after applying `result`: the new version, each channel that was changed under
 * the words above and how many were left unchanged. A failure is called out first, with its
 * reason, and says the new version is active and the failed channel kept its definition.
 */
export function describeApply(
  result: ApplyResult,
  nameOf: (channelId: string) => string,
  heading: { done: string; partial: string; lead?: string },
): ApplyReport {
  const changed = sortedOutcomes(result.outcomes).filter(
    (each) => each.outcome !== "unchanged" && each.outcome !== "failed",
  );
  const groups = ORDER.filter((kind) => kind !== "unchanged" && kind !== "failed")
    .map((kind) => {
      const names = changed
        .filter((each) => each.outcome === kind)
        .map((each) => nameOf(each.channelId));
      return names.length > 0 ? `${capitalized(kind)}: ${names.join(", ")}.` : null;
    })
    .filter((line): line is string => line !== null);
  const unchanged = result.outcomes.filter((each) => each.outcome === "unchanged").length;
  const rest = [
    ...(groups.length > 0 ? groups : ["No channel needed a change."]),
    `${unchanged} unchanged.`,
  ].join(" ");
  if (result.failures.length === 0) {
    return {
      tone: "success",
      title: heading.done,
      description: heading.lead ? `${heading.lead} ${rest}` : rest,
    };
  }
  const failed = result.failures
    .map(({ channelId, reason }) => `${nameOf(channelId)}: ${reason}`)
    .join("; ");
  return {
    tone: "warning",
    title: heading.partial,
    description: `Version ${result.version} is active, but ${result.failures.length === 1 ? "this channel was" : "these channels were"} not changed and keep${result.failures.length === 1 ? "s" : ""} the previous definition (the next apply retries): ${failed}. ${rest}`,
  };
}
