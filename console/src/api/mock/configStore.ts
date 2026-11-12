import { clone } from "@/lib/clone";
import type { ChannelOutcome, ConfigDocument, ConfigVersionSummary } from "../types";
import { buildVersionHistory, type StoredVersion } from "./configHistory";
import { iso } from "./util";

/** The active `runtime.json` and the versions kept for rollback. */
export class ConfigStore {
  private current: ConfigDocument;
  private readonly versions: StoredVersion[];

  constructor(initial: ConfigDocument, nowMs: number) {
    this.current = clone(initial);
    this.versions = buildVersionHistory(initial, nowMs);
  }

  get version(): number {
    return this.current.version;
  }

  /** The live document, for the mock's own reads; callers must not mutate it. */
  get live(): ConfigDocument {
    return this.current;
  }

  get(): ConfigDocument {
    return clone(this.current);
  }

  list(): ConfigVersionSummary[] {
    return this.versions
      .map((stored) => ({
        ...stored.summary,
        current: stored.summary.version === this.current.version,
      }))
      .reverse();
  }

  getVersion(version: number): ConfigDocument | undefined {
    if (version === this.current.version) return this.get();
    const stored = this.versions.find((v) => v.summary.version === version);
    return stored === undefined ? undefined : clone(stored.document);
  }

  /** Makes `document` the active version `current + 1` and records who applied it, why and what it did to each channel. */
  commit(
    document: ConfigDocument,
    note: string,
    appliedBy: string,
    nowMs: number,
    outcomes: ChannelOutcome[],
  ): number {
    const version = this.current.version + 1;
    this.current = { ...clone(document), version };
    this.versions.push({
      summary: {
        version,
        appliedAt: iso(nowMs),
        appliedBy,
        note,
        endpoints: this.current.endpoints.length,
        channels: this.current.channels.length,
        verifications: [],
        outcomes,
        current: true,
      },
      document: clone(this.current),
    });
    return version;
  }

  /** Signs a kept version off; undefined when it is not kept. */
  verify(
    version: number,
    note: string,
    by: string,
    nowMs: number,
  ): ConfigVersionSummary | undefined {
    const stored = this.versions.find((v) => v.summary.version === version);
    if (stored === undefined) return undefined;
    // A new array, so a list read earlier is not changed behind its holder's back.
    stored.summary = {
      ...stored.summary,
      verifications: [...stored.summary.verifications, { by, at: iso(nowMs), note }],
    };
    return { ...stored.summary, current: version === this.current.version };
  }
}
