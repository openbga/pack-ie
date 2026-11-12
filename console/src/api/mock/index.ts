import { toEngineError } from "@/lib/errors";
import type { EngineApi, Unsubscribe } from "../engine";
import type {
  Alert,
  ApplyResult,
  AuditExportRequest,
  AuditExportResult,
  AuditPage,
  AuditQuery,
  AuditVerification,
  BackupStatus,
  BackupSummary,
  BodyRequest,
  ChannelDetail,
  ConfigDocument,
  ConfigVersionSummary,
  Delivery,
  EndpointDetail,
  EndpointTestRequest,
  EngineCapabilities,
  EngineError,
  EngineInfo,
  EngineSettings,
  LogEvent,
  LogFollowNotice,
  LogPage,
  LogQuery,
  MessageBody,
  MessageDetail,
  MessageExportRequest,
  MessageExportResult,
  MessageFields,
  MessagePage,
  MessageQuery,
  NotificationResult,
  ProbeReport,
  ReportFile,
  ResendRequest,
  ResendResult,
  SavedReport,
  SecretInfo,
  SerialPortInfo,
  Snapshot,
  TrafficSegment,
  TransmissionQuery,
  TransmissionReport,
  ValidationResult,
} from "../types";
import * as audited from "./audited";
import { downloadText } from "./download";
import { MockHost } from "./mockHost";
import { sendTestNotifications } from "./notifications";
import { probeEndpoint } from "./probe";
import { serialPortsOf } from "./serialPorts";
import { engineInfo, engineSettings } from "./settings";
import { transmissionReport } from "./transmission";
import { MockWorld } from "./world";

export interface MockOptions {
  /** Milliseconds of simulated latency per request; 0 for tests. Default: realistic 80-300. */
  latency?: "realistic" | "none";
  /** Starts with no engine reachable (CON-01 demo); `startService` brings it up. */
  offline?: boolean;
  /** Seed of all generated data. Default: a fixed number, so data is stable between runs. */
  seed?: number;
  /** Messages in the retained history. Default 412_000; tests pass small numbers. */
  messageCount?: number;
  /** Events in the log ring. Default 20_000. */
  logRingSize?: number;
  /** Clock in milliseconds since the epoch. Default `Date.now`. */
  now?: () => number;
  /** A freshly installed engine: no endpoints, channels, messages or alerts. */
  empty?: boolean;
  /** No endpoint fails by chance: the same states every run, for journeys that assert on them. */
  steady?: boolean;
  /** Every read and command rejects with an internal error (the error states of the pages). */
  failReads?: boolean;
  /** A fixed latency for every request, in milliseconds (the loading states of the pages). */
  latencyMs?: number;
}

const DEFAULT_SEED = 20_260_611;

/**
 * A synthetic engine behind the `EngineApi`: one lab computer with ~36 channels in every state,
 * hundreds of thousands of lazily generated messages, a log ring, live traffic and a versioned
 * configuration. Everything is deterministic for a seed except the timing of live events.
 */
export class MockEngineApi extends MockHost implements EngineApi {
  /** The mock is its own engine: messages, secrets and alert history all live here. */
  readonly capabilities: EngineCapabilities = {
    messages: "engine",
    secretsWritable: true,
    alertHistory: true,
  };

  /** How many times the console asked for a body or the parsed fields (journeys count them). */
  readonly contentReads = { body: 0, fields: 0 };
  private readonly logRingSize: number;

  constructor(options: MockOptions = {}) {
    const seed = options.seed ?? DEFAULT_SEED;
    const now = options.now ?? (() => Date.now());
    const logRingSize = options.logRingSize ?? 20_000;
    const world = new MockWorld({
      seed,
      messageCount: options.empty ? 0 : (options.messageCount ?? 412_000),
      empty: options.empty ?? false,
      steady: options.steady ?? false,
      logRingSize,
      now,
    });
    world.online = !options.offline;
    super(world, {
      latency: options.latency ?? "realistic",
      fixedLatencyMs: options.latencyMs ?? null,
      failReads: options.failReads ?? false,
      now,
      seed,
    });
    this.logRingSize = logRingSize;
  }

  // Snapshots

  getSnapshot(): Promise<Snapshot> {
    return this.request(() => this.world.snapshot());
  }

  subscribeSnapshots(onSnapshot: (snapshot: Snapshot) => void): Unsubscribe {
    return this.world.subscribe(onSnapshot);
  }

  // Channels and endpoints

  getChannel(id: string): Promise<ChannelDetail> {
    return this.request(() => this.world.channelDetail(id));
  }

  getEndpoint(id: string): Promise<EndpointDetail> {
    return this.request(() => this.world.endpointDetail(id));
  }

  // Setup checks

  listSerialPorts(): Promise<SerialPortInfo[]> {
    return this.request(() => {
      if (this.serialPortsFail)
        this.world.fail("internal", "The serial ports could not be listed.");
      return serialPortsOf(this.world);
    });
  }

  testEndpoint(request: EndpointTestRequest): Promise<ProbeReport> {
    return this.request(() => probeEndpoint(this.world, request));
  }

  testNotification(): Promise<NotificationResult[]> {
    return this.request(() => sendTestNotifications(this.world));
  }

  pauseChannel(id: string): Promise<void> {
    return this.request(() => audited.channelCommand(this.world, "pause", id));
  }

  resumeChannel(id: string): Promise<void> {
    return this.request(() => audited.channelCommand(this.world, "resume", id));
  }

  restartChannel(id: string): Promise<void> {
    return this.request(() => audited.channelCommand(this.world, "restart", id));
  }

  subscribeTraffic(
    channelId: string,
    onSegments: (segments: TrafficSegment[]) => void,
    onError?: (error: EngineError) => void,
  ): Unsubscribe {
    if (!this.world.online) return () => {};
    try {
      return this.world.traffic(channelId, onSegments);
    } catch (error) {
      // The engine refuses a second viewer after answering; report it the way the IPC client does.
      const refusal = toEngineError(error);
      queueMicrotask(() => onError?.(refusal));
      return () => {};
    }
  }

  // Messages

  listMessages(query: MessageQuery): Promise<MessagePage> {
    return this.request(() => audited.listMessages(this.world, query));
  }

  getMessage(id: string): Promise<MessageDetail> {
    return this.request(() => this.world.messages.get(id));
  }

  getMessageFields(id: string): Promise<MessageFields> {
    this.contentReads.fields += 1;
    return this.request(() => audited.messageFields(this.world, id));
  }

  getMessageBody(request: BodyRequest): Promise<MessageBody> {
    this.contentReads.body += 1;
    return this.request(() => audited.messageBody(this.world, request));
  }

  resendMessage(request: ResendRequest): Promise<ResendResult> {
    return this.request(() => {
      const result = audited.resend(this.world, request);
      const queued = result.destinations.filter((d) => d.rejected === null).length;
      this.logMessageAction(
        request.messageId,
        `resend requested (${request.mode}): ${queued} of ${result.destinations.length} destinations accepted`,
      );
      return result;
    });
  }

  closeDelivery(deliveryId: string, reason: string): Promise<Delivery> {
    return this.request(() => {
      const delivery = audited.closeDelivery(this.world, deliveryId, reason);
      this.logMessageAction(delivery.messageId, "delivery closed by operator");
      return delivery;
    });
  }

  exportMessages(request: MessageExportRequest): Promise<MessageExportResult | null> {
    return this.request(() => {
      const { content, rows } = audited.exportMessages(this.world, request);
      const name = `packie-messages.${request.format}`;
      downloadText(name, content);
      return { savedTo: name, rows };
    });
  }

  verifyTransmission(query: TransmissionQuery): Promise<TransmissionReport> {
    return this.request(() => transmissionReport(this.world, query));
  }

  private logMessageAction(messageId: string, message: string): void {
    this.world.logs.append({
      level: "info",
      channelId: null,
      endpointId: null,
      messageId,
      message,
      fields: {},
    });
  }

  // Logs

  queryLogs(query: LogQuery, page: { before: number | null; limit: number }): Promise<LogPage> {
    return this.request(() => this.world.queryLogs(query, page));
  }

  followLogs(
    query: LogQuery,
    onEvents: (events: LogEvent[], notice?: LogFollowNotice) => void,
  ): Unsubscribe {
    if (!this.world.online) return () => {};
    return this.world.logs.follow(query, onEvents);
  }

  // Alerts

  listAlerts(state: "open" | "cleared" | "all"): Promise<Alert[]> {
    return this.request(() => this.world.listAlerts(state));
  }

  // Configuration

  getConfig(): Promise<ConfigDocument> {
    return this.request(() => this.world.config.get());
  }

  validateConfig(document: ConfigDocument): Promise<ValidationResult> {
    return this.request(() => this.world.changes.validate(document));
  }

  applyConfig(document: ConfigDocument, note: string): Promise<ApplyResult> {
    return this.request(() => this.world.changes.apply(document, note));
  }

  listConfigVersions(): Promise<ConfigVersionSummary[]> {
    return this.request(() => this.world.changes.versions());
  }

  getConfigVersion(version: number): Promise<ConfigDocument> {
    return this.request(() => this.world.changes.versionDocument(version));
  }

  rollbackConfig(version: number, reason: string): Promise<ApplyResult> {
    return this.request(() => this.world.changes.rollback(version, reason));
  }

  previewRollback(version: number): Promise<ValidationResult> {
    return this.request(() => this.world.changes.previewRollback(version));
  }

  verifyConfigVersion(version: number, note: string): Promise<ConfigVersionSummary> {
    return this.request(() => this.world.changes.verifyVersion(version, note));
  }

  /** Test hook: another client (the CLI, say) applies the active document, making a new version. */
  async applyElsewhere(): Promise<void> {
    await this.applyConfig(await this.getConfig(), "applied elsewhere (test hook)");
  }

  // Audit log

  getAuditEvents(query: AuditQuery): Promise<AuditPage> {
    return this.request(() => this.world.audit.query(query));
  }

  exportAudit(request: AuditExportRequest): Promise<AuditExportResult | null> {
    return this.request(() => {
      const { content, rows } = this.world.audit.exportText(request.filter, request.format);
      const name = `packie-audit.${request.format === "csv" ? "csv" : "jsonl"}`;
      downloadText(name, content);
      return { savedTo: name, rows };
    });
  }

  verifyAudit(): Promise<AuditVerification> {
    return this.request(() => this.world.audit.verify());
  }

  saveReportFile(file: ReportFile): Promise<SavedReport | null> {
    return this.request(() => {
      audited.recordClientExport(this.world, file);
      downloadText(file.name, file.content);
      return { savedTo: file.name };
    });
  }

  // Backup

  createBackup(): Promise<BackupSummary> {
    return this.request(() =>
      this.world.audit.command({ action: "backup_create", objectType: "backup" }, () => {
        const summary = this.world.backup.create();
        this.world.publish();
        return summary;
      }),
    );
  }

  getBackupStatus(): Promise<BackupStatus> {
    return this.request(() => this.world.backup.status());
  }

  // Settings

  getEngineSettings(): Promise<EngineSettings> {
    return this.request(() =>
      engineSettings(this.logRingSize, this.world.notifications.settings()),
    );
  }

  listSecrets(): Promise<SecretInfo[]> {
    return this.request(() => this.world.secrets.list(this.world.config.live));
  }

  setSecret(name: string, value: string): Promise<void> {
    return this.request(() => {
      const fieldErrors = [];
      if (name.trim() === "") fieldErrors.push({ field: "name", message: "A name is required." });
      if (value === "") fieldErrors.push({ field: "value", message: "A value is required." });
      if (fieldErrors.length > 0)
        this.world.fail("validation", "The secret needs a name and a value.", fieldErrors);
      audited.secret(this.world, "secret_set", name.trim(), () =>
        this.world.secrets.set(name.trim()),
      );
    });
  }

  deleteSecret(name: string): Promise<void> {
    return this.request(() => {
      if (!this.world.secrets.has(name))
        this.world.fail("not_found", `Secret "${name}" does not exist.`);
      audited.secret(this.world, "secret_delete", name, () => this.world.secrets.delete(name));
    });
  }

  getEngineInfo(): Promise<EngineInfo> {
    return this.request(() => engineInfo());
  }
}
