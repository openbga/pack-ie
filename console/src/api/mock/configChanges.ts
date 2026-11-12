import type {
  ApplyPlan,
  ApplyResult,
  ChannelOutcome,
  ConfigDocument,
  ConfigVersionSummary,
  ValidationResult,
} from "../types";
import { MOCK_ACTOR } from "./audit";
import { diffDocuments, outcomesOfPlan, planDocument } from "./configDiff";
import { buildLogChannels } from "./logGenerator";
import { validateDocument } from "./validate";
import type { MockWorld } from "./world";

const NOTE_LIMIT = 500;

/** What the engine does with configuration documents: validating, applying and rolling back, versions. */
export class ConfigChanges {
  /** The version of an apply in flight, for the node snapshot. */
  pendingVersion: number | null = null;
  /** A channel the next apply fails to change (journeys: the partial path). */
  private nextApplyFailure: { channelId: string; reason: string } | null = null;

  constructor(private readonly world: MockWorld) {}

  validate(document: ConfigDocument): ValidationResult {
    const issues = validateDocument(document, this.world.secrets.names());
    const ok = !issues.some((issue) => issue.severity === "error");
    // The engine answers a document with errors with an empty plan.
    const plan = ok
      ? planDocument(this.world.config.live, document)
      : { channels: [], endpoints: [], alertsChanged: false };
    return { ok, issues, plan };
  }

  /** Why a change is made: required, 1 to 500 characters (configuration.md section 10). */
  private requireNote(note: string): string {
    const trimmed = note.trim();
    if (trimmed === "" || trimmed.length > NOTE_LIMIT) {
      this.world.fail("validation", "Say why in the change note (1 to 500 characters).", [
        { field: "note", message: "Enter a change note of 1 to 500 characters." },
      ]);
    }
    return trimmed;
  }

  apply(document: ConfigDocument, note: string): ApplyResult {
    return this.world.audit.command(
      { action: "config_apply", details: { base_version: document.version, note: note.trim() } },
      () => this.applyDocument(document, this.requireNote(note)),
    );
  }

  private applyDocument(document: ConfigDocument, note: string): ApplyResult {
    if (document.version !== this.world.config.version) {
      this.world.fail(
        "conflict",
        "The configuration changed since it was loaded. Reload it and try again.",
      );
    }
    const result = this.validate(document);
    const errors = result.issues.filter((issue) => issue.severity === "error");
    if (errors.length > 0) {
      this.world.fail(
        "validation",
        `The configuration has ${errors.length} error${errors.length === 1 ? "" : "s"}; nothing was applied.`,
        errors.map((issue) => ({ field: issue.path, message: issue.message })),
      );
    }
    // Subscribers see the apply in flight (pending version) before the new version is active.
    this.pendingVersion = this.world.config.version + 1;
    this.world.publish();
    try {
      return this.commitDocument(document, note);
    } finally {
      this.pendingVersion = null;
    }
  }

  /** Makes the next apply or rollback end `partial`: `channelId` fails with `reason`. */
  failNextApply(channelId: string, reason: string): void {
    this.nextApplyFailure = { channelId, reason };
  }

  /** The engine's outcome for every channel; a failure forced by `failNextApply` replaces its channel's. */
  private applyOutcomes(plan: ApplyPlan): ChannelOutcome[] {
    const outcomes = outcomesOfPlan(plan);
    const failure = this.nextApplyFailure;
    this.nextApplyFailure = null;
    if (failure === null) return outcomes;
    return outcomes.map((each) =>
      each.channelId === failure.channelId
        ? { channelId: each.channelId, outcome: "failed", reason: failure.reason }
        : each,
    );
  }

  private commitDocument(document: ConfigDocument, note: string): ApplyResult {
    const diff = diffDocuments(this.world.config.live, document);
    const outcomes = this.applyOutcomes(planDocument(this.world.config.live, document));
    // A failed channel keeps its previous definition and instance: it is not restarted.
    const restarted = outcomes
      .filter((each) => each.outcome === "restarted")
      .map((each) => each.channelId);
    const failures = outcomes.flatMap((each) =>
      each.outcome === "failed" ? [{ channelId: each.channelId, reason: each.reason ?? "" }] : [],
    );
    const version = this.world.config.commit(
      document,
      note,
      MOCK_ACTOR.name,
      this.world.now(),
      outcomes,
    );
    this.pendingVersion = null;
    this.world.runtime.sync(
      this.world.config.live,
      restarted,
      diff.changedEndpoints.map((e) => e.id),
    );
    this.world.logs.setChannels(buildLogChannels(this.world.config.live));
    this.world.logs.append({
      level: failures.length > 0 ? "warn" : "info",
      channelId: null,
      endpointId: null,
      messageId: null,
      message:
        failures.length > 0
          ? `configuration version ${version} applied, but not for ${failures.length} channel(s)`
          : `configuration version ${version} applied`,
      fields: { restarted: String(restarted.length), failed: String(failures.length) },
    });
    this.world.publish();
    return {
      version,
      outcomes,
      restartedChannelIds: restarted,
      unchangedChannelCount: outcomes.filter((each) => each.outcome === "unchanged").length,
      failures,
    };
  }

  versions(): ConfigVersionSummary[] {
    return this.world.config.list();
  }

  versionDocument(version: number): ConfigDocument {
    return (
      this.world.config.getVersion(version) ??
      this.world.fail("not_found", `Version ${version} is not kept.`)
    );
  }

  rollback(version: number, reason: string): ApplyResult {
    if (version === this.world.config.version)
      this.world.fail("conflict", "That version is already active.");
    const document = this.versionDocument(version);
    const note = `rollback of ${version}${reason.trim() ? `: ${reason.trim()}` : ""}`;
    return this.world.audit.command(
      { action: "config_rollback", details: { base_version: this.world.config.version, note } },
      () => this.applyDocument({ ...document, version: this.world.config.version }, note),
    );
  }

  /** The engine's plan for rolling back to `version` over the active one; changes nothing. */
  previewRollback(version: number): ValidationResult {
    if (version === this.world.config.version)
      this.world.fail("conflict", "That version is already active.");
    return this.validate({ ...this.versionDocument(version), version: this.world.config.version });
  }

  verifyVersion(version: number, note: string): ConfigVersionSummary {
    const trimmed = this.requireNote(note);
    return this.world.audit.command(
      {
        action: "config_verify",
        objectType: "config_version",
        objectId: String(version),
        details: { note: trimmed },
      },
      () =>
        this.world.config.verify(version, trimmed, MOCK_ACTOR.name, this.world.now()) ??
        this.world.fail("not_found", `Version ${version} is not kept.`),
    );
  }
}
