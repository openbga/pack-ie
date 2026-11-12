/**
 * The configuration documents: the active one, stored versions, validate, apply, verify and
 * roll back. Every apply names the active version it was edited from; the engine refuses a
 * stale one.
 */

import type { ApplyResult, ConfigDocument, ConfigVersionSummary, ValidationResult } from "../types";
import type { IpcClient } from "./client";
import type { ConfigCache } from "./configCache";
import { applyResult, documentFromWire, documentToWire, validationResult } from "./configDocument";
import { engineError } from "./errors";
import { requireNote, versionFromIpc } from "./versions";
import type { WireConfig } from "./wire";

export class ConfigApi {
  constructor(
    private readonly client: IpcClient,
    private readonly configs: ConfigCache,
  ) {}

  async get(): Promise<ConfigDocument> {
    return documentFromWire(await this.configs.fetchActive());
  }

  async version(version: number): Promise<ConfigDocument> {
    return documentFromWire(await this.configs.stored(version));
  }

  /** The document `document` was edited from, so that what the console does not model survives. */
  private originalOf(document: ConfigDocument): Promise<WireConfig | undefined> {
    return this.configs.stored(document.version).catch(() => undefined);
  }

  async validate(document: ConfigDocument): Promise<ValidationResult> {
    const converted = documentToWire(document, await this.originalOf(document));
    const result = await this.client.request(
      { type: "validate_config", document: converted.document },
      "validation",
      converted.document,
    );
    return validationResult(converted.issues, result.problems, converted.document, result.diff);
  }

  async apply(document: ConfigDocument, note: string): Promise<ApplyResult> {
    requireNote(note);
    const converted = documentToWire(document, await this.originalOf(document));
    if (converted.issues.length > 0) {
      throw engineError(
        "validation",
        "The configuration has errors.",
        converted.issues.map(({ path, message }) => ({ field: path, message })),
      );
    }
    return this.put(converted.document, document.version, note);
  }

  /** `baseVersion` is the active version the document was edited from. */
  private async put(document: WireConfig, baseVersion: number, note: string): Promise<ApplyResult> {
    const applied = await this.client.request(
      {
        type: "put_config",
        document,
        base_version: baseVersion,
        note: note.trim(),
      },
      "config_applied",
      document,
    );
    this.configs.invalidate();
    return applyResult(applied.config_version, applied.outcomes);
  }

  async versions(): Promise<ConfigVersionSummary[]> {
    const [history, active] = await Promise.all([
      this.client.request({ type: "get_config_history" }, "config_history"),
      this.configs.fetchActive(),
    ]);
    return history.versions.map((v) => versionFromIpc(v, active.config_version ?? 0));
  }

  async verifyVersion(version: number, note: string): Promise<ConfigVersionSummary> {
    requireNote(note);
    const [verified, active] = await Promise.all([
      this.client.request(
        { type: "verify_config_version", config_version: version, note: note.trim() },
        "config_version_verified",
      ),
      this.configs.fetchActive(),
    ]);
    return versionFromIpc(verified.version, active.config_version ?? 0);
  }

  /** The engine's validation of the stored `version` as a new document over the active one. */
  async previewRollback(version: number): Promise<ValidationResult> {
    const [target, active] = await Promise.all([
      this.configs.stored(version),
      this.configs.fetchActive(),
    ]);
    const document = { ...target, config_version: active.config_version ?? 0 };
    const result = await this.client.request(
      { type: "validate_config", document },
      "validation",
      document,
    );
    return validationResult([], result.problems, document, result.diff);
  }

  async rollback(version: number, reason: string): Promise<ApplyResult> {
    requireNote(reason);
    const [target, active] = await Promise.all([
      this.configs.stored(version),
      this.configs.fetchActive(),
    ]);
    const base = active.config_version ?? 0;
    return this.put(
      { ...target, config_version: base },
      base,
      `rollback of ${version}: ${reason.trim()}`,
    );
  }
}
