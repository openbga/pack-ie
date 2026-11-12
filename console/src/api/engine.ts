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
  ConnectionStatus,
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
} from "./types";

/** Stops a subscription. Calling it twice is harmless. */
export type Unsubscribe = () => void;

/**
 * Everything the console asks of the engine. One method per IPC request or subscription
 * (architecture §1). Every request method rejects with an `EngineError`
 * (`toEngineError` normalizes anything else). The mock implementation lives in `./mock`; the
 * IPC-backed one replaces it without touching the pages.
 *
 * Reads that page or filter do so on the engine (server side): the console never holds more
 * than one page of messages or logs.
 */
export interface EngineApi {
  readonly capabilities: EngineCapabilities;

  // Connection (CON-01)
  getConnection(): Promise<ConnectionStatus>;
  /** Streams the connection state: the current one first, then every change. */
  subscribeConnection(onStatus: (status: ConnectionStatus) => void): Unsubscribe;
  /**
   * Asks the OS to start the installed service and resolves with what it said. Rejects with
   * the reason when it cannot (for example, "not implemented in this build").
   */
  startService(): Promise<string>;

  // Snapshots, published every second and on change
  getSnapshot(): Promise<Snapshot>;
  subscribeSnapshots(onSnapshot: (snapshot: Snapshot) => void): Unsubscribe;

  // Channels and endpoints
  getChannel(id: string): Promise<ChannelDetail>;
  getEndpoint(id: string): Promise<EndpointDetail>;

  // Setup checks (diagnostics.md §10): read-only, never send data to an instrument or system.
  /** The serial ports the OS reports, sorted by name, each with the configured endpoints naming it. Opens no port. */
  listSerialPorts(): Promise<SerialPortInfo[]>;
  /**
   * Probes an endpoint definition that need not be saved (the editor's current settings): each
   * step with its time, then a summary. A definition the engine's endpoint rules reject
   * rejects with `validation` and field errors at `endpoints[0].<field>`. Audited by the engine.
   */
  testEndpoint(request: EndpointTestRequest): Promise<ProbeReport>;
  /** Sends one test message to every notification target the engine has; empty when none is configured. Audited. */
  testNotification(): Promise<NotificationResult[]>;

  // Commands
  pauseChannel(id: string): Promise<void>;
  resumeChannel(id: string): Promise<void>;
  restartChannel(id: string): Promise<void>;

  // Live traffic: subscribing turns the channel's tap on, unsubscribing turns it off.
  subscribeTraffic(
    channelId: string,
    onSegments: (segments: TrafficSegment[]) => void,
    /** The engine refused or ended the transcript, such as "Transcript in use by another client". */
    onError?: (error: EngineError) => void,
  ): Unsubscribe;

  // Messages (the engine's store; a build without one rejects these with `not_implemented`)
  listMessages(query: MessageQuery): Promise<MessagePage>;
  /** Everything about one message except its bodies. */
  getMessage(id: string): Promise<MessageDetail>;
  /**
   * The original's parsed fields: message content, asked for only by an explicit "Show content",
   * which the engine records in the audit log.
   */
  getMessageFields(id: string): Promise<MessageFields>;
  /** One body, rendered by the engine as text or hex; asked for only by an explicit action. */
  getMessageBody(request: BodyRequest): Promise<MessageBody>;
  /**
   * Resends to the requested destinations. A precondition that fails for one destination is
   * in its entry of the result, not an error; the call rejects only when the message is not
   * found or the store is not available.
   */
  resendMessage(request: ResendRequest): Promise<ResendResult>;
  /**
   * Closes one delivery that needs attention with an operator's reason, 1 to 500 characters
   * (MSG-04). Does not contact the destination. Resolves with the delivery as it now is.
   */
  closeDelivery(deliveryId: string, reason: string): Promise<Delivery>;
  /**
   * Exports the messages the filter selects, oldest first, to a file the operator picks in a
   * save dialog, writing the engine's chunks as they arrive (a failure leaves no partial file).
   * Resolves with null when the dialog is cancelled. The engine audits it; with
   * `includeBodies` it is a content view.
   */
  exportMessages(request: MessageExportRequest): Promise<MessageExportResult | null>;
  /**
   * What one channel received in a period and what each destination confirmed (compliance
   * P7). The engine audits it and lists at most 10,000 deliveries (`truncated`).
   */
  verifyTransmission(query: TransmissionQuery): Promise<TransmissionReport>;

  // Logs
  /** Newest first; `before` is a `LogPage.olderCursor`. */
  queryLogs(query: LogQuery, page: { before: number | null; limit: number }): Promise<LogPage>;
  /**
   * Streams events matching `query` that arrive after subscribing, oldest first. When the
   * connection to the engine returns after an outage, the events missed are read from the
   * engine's log and delivered once each, with a `notice` saying what the outage cost; `events`
   * may then be empty.
   */
  followLogs(
    query: LogQuery,
    onEvents: (events: LogEvent[], notice?: LogFollowNotice) => void,
  ): Unsubscribe;

  // Alerts
  listAlerts(state: "open" | "cleared" | "all"): Promise<Alert[]>;

  // Configuration: validated before apply; an invalid document leaves the active one in place.
  getConfig(): Promise<ConfigDocument>;
  validateConfig(document: ConfigDocument): Promise<ValidationResult>;
  applyConfig(document: ConfigDocument, note: string): Promise<ApplyResult>;
  listConfigVersions(): Promise<ConfigVersionSummary[]>;
  getConfigVersion(version: number): Promise<ConfigDocument>;
  /**
   * Applies an older version as a new one. The note is `rollback of <version>: <reason>`; the
   * reason (1 to 450 characters) says why.
   */
  rollbackConfig(version: number, reason: string): Promise<ApplyResult>;
  /**
   * The engine's validation and plan for rolling back to `version` over the active one:
   * channels restarted, updated in place, started, removed or untouched. Changes nothing.
   */
  previewRollback(version: number): Promise<ValidationResult>;
  /**
   * Signs a saved version off as verified after its change, with the lab's reference (1 to 500
   * characters). Resolves with the version as it now is.
   */
  verifyConfigVersion(version: number, note: string): Promise<ConfigVersionSummary>;

  // Audit log (docs/spec/audit.md): reading it is itself recorded.
  /** Newest first, one page; `nextBeforeSeq` of the answer fetches the next. */
  getAuditEvents(query: AuditQuery): Promise<AuditPage>;
  /**
   * Exports every matching event to a file the operator picks in a save dialog. Resolves with
   * null when the dialog is cancelled.
   */
  exportAudit(request: AuditExportRequest): Promise<AuditExportResult | null>;
  /** Walks the hash chain of every audit file. */
  verifyAudit(): Promise<AuditVerification>;

  /**
   * Saves a report the console built (the change report, a transmission CSV) to a file the
   * operator picks in a save dialog. Resolves with null when the dialog is cancelled.
   */
  saveReportFile(file: ReportFile): Promise<SavedReport | null>;

  // Backup (docs/spec/backup.md)
  /** Takes a backup now, into a new directory under `backup.directory`. */
  createBackup(): Promise<BackupSummary>;
  getBackupStatus(): Promise<BackupStatus>;

  // Settings
  getEngineSettings(): Promise<EngineSettings>;
  listSecrets(): Promise<SecretInfo[]>;
  /** Stores a value under `name`; the value is never read back. */
  setSecret(name: string, value: string): Promise<void>;
  deleteSecret(name: string): Promise<void>;
  getEngineInfo(): Promise<EngineInfo>;
}
