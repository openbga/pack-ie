// A saved version of the configuration (`VersionInfo`) as the history shows it.
import type { ChannelOutcome as RawOutcome, VersionInfo } from "@/ipc/generated";
import type { ChannelOutcome, ConfigVersionSummary } from "../types";
import { engineError } from "./errors";

export function outcomeFromIpc(raw: RawOutcome): ChannelOutcome {
  return raw.outcome === "failed"
    ? { channelId: raw.channel_id, outcome: "failed", reason: raw.reason }
    : { channelId: raw.channel_id, outcome: raw.outcome };
}

/** `activeVersion` is the one the engine runs, which the history marks as current. */
export function versionFromIpc(raw: VersionInfo, activeVersion: number): ConfigVersionSummary {
  return {
    version: raw.config_version,
    appliedAt: raw.applied_at ?? raw.generated_at,
    appliedBy: raw.applied_by?.name ?? null,
    note: raw.note,
    endpoints: raw.endpoints,
    channels: raw.channels,
    verifications: raw.verifications.map((each) => ({
      by: each.verified_by.name,
      at: each.verified_at,
      note: each.note,
    })),
    outcomes: raw.outcomes.map(outcomeFromIpc),
    current: raw.config_version === activeVersion,
  };
}

/** The longest note the engine takes (configuration.md section 10). */
const NOTE_MAX = 500;

/** Change control needs a reason (1 to 500 characters); refused here so nothing is sent without one. */
export function requireNote(note: string): void {
  const trimmed = note.trim();
  if (trimmed === "" || trimmed.length > NOTE_MAX) {
    throw engineError("validation", "Say why in the note (1 to 500 characters).", [
      { field: "note", message: "Enter a note of 1 to 500 characters." },
    ]);
  }
}
