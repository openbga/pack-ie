import type { IpcClient } from "./client";
import type { WireConfig } from "./wire";

const KEPT_VERSIONS = 8;
const RETRY_AFTER_FAILURE_MS = 5000;

/**
 * The runtime documents read from the engine, by `config_version`. Snapshots name a version
 * (`config.active_version`); the mapping layer needs its document for the backlog threshold
 * that queue pressure is measured against and for endpoint addresses a snapshot leaves out, and
 * writing needs the document an edit started from.
 */
export class ConfigCache {
  private readonly documents = new Map<number, WireConfig>();
  private inflight: Promise<WireConfig> | null = null;
  private failedAt: number | null = null;
  private generation = 0;
  private active: number | null = null;

  constructor(
    private readonly client: IpcClient,
    private readonly now: () => number = Date.now,
  ) {}

  /** The cached document of `version`, if any. */
  get(version: number): WireConfig | null {
    return this.documents.get(version) ?? null;
  }

  /** The active version as last seen (a snapshot, or the active document). */
  get activeVersion(): number | null {
    return this.active;
  }

  noteActive(version: number): void {
    this.active = version;
  }

  /** Forgets everything; the next read goes to the engine (after an apply). */
  invalidate(): void {
    this.documents.clear();
    this.inflight = null;
    this.failedAt = null;
    this.active = null;
    this.generation += 1;
  }

  store(document: WireConfig): void {
    const version = document.config_version ?? 0;
    this.documents.delete(version);
    this.documents.set(version, document);
    for (const version of this.documents.keys()) {
      if (this.documents.size <= KEPT_VERSIONS) break;
      this.documents.delete(version);
    }
  }

  /** The active document: the cached one when the active version is known, else fresh. */
  async current(): Promise<WireConfig> {
    const cached = this.active === null ? null : this.get(this.active);
    return cached ?? this.fetchActive();
  }

  /** The active document, fresh from the engine; concurrent callers share one request. */
  fetchActive(): Promise<WireConfig> {
    if (this.inflight !== null) return this.inflight;
    const generation = this.generation;
    const load = this.client
      .request({ type: "get_config" }, "config")
      .then(({ document }) => {
        this.store(document);
        if (generation === this.generation) this.active = document.config_version ?? 0;
        return document;
      })
      .finally(() => {
        if (generation === this.generation) this.inflight = null;
      });
    this.inflight = load;
    return load;
  }

  /** The document of `version`: cached, else its stored copy. */
  async stored(version: number): Promise<WireConfig> {
    const cached = this.get(version);
    if (cached !== null) return cached;
    const { document } = await this.client.request(
      { type: "get_config_version", config_version: version },
      "config",
    );
    this.store(document);
    return document;
  }

  /**
   * The document in effect at `version` (a snapshot's `config.active_version`): cached, or
   * the engine's active document. Null when the engine cannot say; after a failure it does
   * not ask again for a few seconds.
   */
  async forSnapshot(version: number): Promise<WireConfig | null> {
    const cached = this.get(version);
    if (cached !== null) return cached;
    if (this.failedAt !== null && this.now() - this.failedAt < RETRY_AFTER_FAILURE_MS) return null;
    try {
      const document = await this.fetchActive();
      this.failedAt = null;
      return (document.config_version ?? 0) === version ? document : this.get(version);
    } catch {
      this.failedAt = this.now();
      return null;
    }
  }
}
