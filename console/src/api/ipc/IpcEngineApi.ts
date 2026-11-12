import type { EngineSnapshot } from "@/ipc/generated";
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
} from "../types";
import { AdminApi } from "./adminApi";
import { alertFromIpc, sortAlerts } from "./alerts";
import type { TauriBridge } from "./bridge";
import { IpcClient } from "./client";
import { ConfigApi } from "./configApi";
import { ConfigCache } from "./configCache";
import { channelDetail, endpointDetail } from "./detail";
import { endpointToWire } from "./endpointConfig";
import { fromServiceError } from "./errors";
import { followLogs, LogPager } from "./logs";
import { MessageApi } from "./messages";
import { connectionStatusFrom } from "./settings";
import {
  atEndpointZero,
  notificationResultFromIpc,
  probeFromIpc,
  serialPortFromIpc,
} from "./setup";
import { snapshotFromIpc } from "./snapshot";
import { subscribeTraffic } from "./traffic";
import type { WireConfig } from "./wire";

/**
 * The `EngineApi` over the engine's IPC protocol, through the Tauri commands behind a
 * `TauriBridge`. Mapping lives in the sibling modules; this class only sequences requests.
 */
export class IpcEngineApi implements EngineApi {
  readonly capabilities: EngineCapabilities = {
    messages: "engine",
    secretsWritable: true,
    alertHistory: true,
  };

  private readonly client: IpcClient;
  private readonly configs: ConfigCache;
  private readonly logPager: LogPager;
  private readonly messageApi: MessageApi;
  private readonly configApi: ConfigApi;
  private readonly adminApi: AdminApi;

  constructor(private readonly bridge: TauriBridge) {
    this.client = new IpcClient(bridge);
    this.configs = new ConfigCache(this.client);
    this.logPager = new LogPager(this.client);
    this.messageApi = new MessageApi(this.client, this.configs, bridge);
    this.configApi = new ConfigApi(this.client, this.configs);
    this.adminApi = new AdminApi(this.client, this.configs, bridge);
  }

  /** Stops the Tauri listeners; tests call it, the app never needs to. */
  dispose(): void {
    this.client.dispose();
  }

  // Connection

  async getConnection(): Promise<ConnectionStatus> {
    return connectionStatusFrom(await this.client.currentConnection());
  }

  subscribeConnection(onStatus: (status: ConnectionStatus) => void): Unsubscribe {
    return this.client.watchConnection((payload) => onStatus(connectionStatusFrom(payload)));
  }

  async startService(): Promise<string> {
    try {
      return await this.bridge.invoke<string>("start_service");
    } catch (error) {
      throw fromServiceError(error);
    }
  }

  // Snapshots

  private async rawSnapshot(): Promise<{ raw: EngineSnapshot; config: WireConfig | null }> {
    const { snapshot } = await this.client.request({ type: "get_snapshot" }, "snapshot");
    const version = snapshot.node.config.active_version;
    if (version === null) return { raw: snapshot, config: null };
    this.configs.noteActive(version);
    return { raw: snapshot, config: await this.configs.forSnapshot(version) };
  }

  async getSnapshot(): Promise<Snapshot> {
    const { raw, config } = await this.rawSnapshot();
    return snapshotFromIpc(raw, config);
  }

  subscribeSnapshots(onSnapshot: (snapshot: Snapshot) => void): Unsubscribe {
    let latest: EngineSnapshot | null = null;
    let waitingFor: number | null = null;
    let live = true;

    const emit = (): void => {
      if (!live || latest === null) return;
      const version = latest.node.config.active_version;
      if (version === null) {
        onSnapshot(snapshotFromIpc(latest, null));
        return;
      }
      this.configs.noteActive(version);
      const config = this.configs.get(version);
      onSnapshot(snapshotFromIpc(latest, config));
      if (config !== null || waitingFor === version) return;
      waitingFor = version;
      void this.configs.forSnapshot(version).then((found) => {
        waitingFor = null;
        if (found !== null) emit();
      });
    };

    const stop = this.client.subscribe({ type: "snapshots" }, (event) => {
      if (event.type !== "snapshot") return;
      latest = event.snapshot;
      emit();
    });
    return () => {
      live = false;
      stop();
    };
  }

  // Channels and endpoints

  async getChannel(id: string): Promise<ChannelDetail> {
    const { raw, config } = await this.rawSnapshot();
    const document = config ?? (await this.configs.current());
    return channelDetail(id, raw, snapshotFromIpc(raw, document), document);
  }

  async getEndpoint(id: string): Promise<EndpointDetail> {
    const { raw, config } = await this.rawSnapshot();
    const document = config ?? (await this.configs.current());
    return endpointDetail(id, snapshotFromIpc(raw, document), document);
  }

  // Setup checks

  async listSerialPorts(): Promise<SerialPortInfo[]> {
    const { ports } = await this.client.request({ type: "list_serial_ports" }, "serial_ports");
    return ports.map(serialPortFromIpc);
  }

  /**
   * Probes the definition as written in the document's wire form. The engine names problems
   * relative to the endpoint (`settings.port_name`); they come back at `endpoints[0]`, which
   * the editor maps to the endpoint's own index.
   */
  async testEndpoint({ endpoint, timeoutSeconds }: EndpointTestRequest): Promise<ProbeReport> {
    const original = await this.configs
      .current()
      .then((config) => config.endpoints?.find((each) => each.endpoint_id === endpoint.id))
      .catch(() => undefined);
    const wire = endpointToWire(endpoint, original);
    const alone: WireConfig = { endpoints: [wire] };
    const report = await this.client
      .request(
        {
          type: "test_endpoint",
          kind: wire.kind,
          settings: wire.settings as Record<string, unknown>,
          timeout_seconds: timeoutSeconds ?? null,
        },
        "endpoint_tested",
      )
      .catch((error: unknown) => {
        throw atEndpointZero(error, alone);
      });
    return probeFromIpc(report);
  }

  async testNotification(): Promise<NotificationResult[]> {
    const { results } = await this.client.request(
      { type: "test_notification" },
      "notification_tested",
    );
    return results.map(notificationResultFromIpc);
  }

  // Commands

  private async command(channelId: string, command: "pause" | "resume" | "restart") {
    await this.client.request({ type: "channel_command", channel_id: channelId, command }, "done");
  }

  pauseChannel(id: string): Promise<void> {
    return this.command(id, "pause");
  }

  resumeChannel(id: string): Promise<void> {
    return this.command(id, "resume");
  }

  restartChannel(id: string): Promise<void> {
    return this.command(id, "restart");
  }

  subscribeTraffic(
    channelId: string,
    onSegments: (segments: TrafficSegment[]) => void,
    onError?: (error: EngineError) => void,
  ): Unsubscribe {
    const channel = async () =>
      (await this.configs.current()).channels?.find((c) => c.channel_id === channelId);
    return subscribeTraffic(this.client, channelId, channel, onSegments, onError);
  }

  // Messages

  listMessages(query: MessageQuery): Promise<MessagePage> {
    return this.messageApi.list(query);
  }

  getMessage(id: string): Promise<MessageDetail> {
    return this.messageApi.get(id);
  }

  getMessageFields(id: string): Promise<MessageFields> {
    return this.messageApi.fields(id);
  }

  getMessageBody(request: BodyRequest): Promise<MessageBody> {
    return this.messageApi.body(request);
  }

  resendMessage(request: ResendRequest): Promise<ResendResult> {
    return this.messageApi.resend(request);
  }

  closeDelivery(deliveryId: string, reason: string): Promise<Delivery> {
    return this.messageApi.closeDelivery(deliveryId, reason);
  }

  exportMessages(request: MessageExportRequest): Promise<MessageExportResult | null> {
    return this.messageApi.export(request);
  }

  verifyTransmission(query: TransmissionQuery): Promise<TransmissionReport> {
    return this.messageApi.verifyTransmission(query);
  }

  // Logs

  queryLogs(query: LogQuery, page: { before: number | null; limit: number }): Promise<LogPage> {
    return this.logPager.query(query, page);
  }

  followLogs(
    query: LogQuery,
    onEvents: (events: LogEvent[], notice?: LogFollowNotice) => void,
  ): Unsubscribe {
    return followLogs(this.client, query, onEvents);
  }

  // Alerts

  /** Open alerts most severe first, then cleared ones newest first. */
  async listAlerts(state: "open" | "cleared" | "all"): Promise<Alert[]> {
    const { alerts } = await this.client.request(
      { type: "get_alerts", include_cleared: state !== "open", since: null },
      "alerts",
    );
    const mapped = alerts.map(alertFromIpc);
    const open = sortAlerts(mapped.filter((alert) => alert.state === "open"));
    const cleared = mapped.filter((alert) => alert.state === "cleared");
    return [...(state === "cleared" ? [] : open), ...(state === "open" ? [] : cleared)];
  }

  // Configuration

  getConfig(): Promise<ConfigDocument> {
    return this.configApi.get();
  }

  getConfigVersion(version: number): Promise<ConfigDocument> {
    return this.configApi.version(version);
  }

  validateConfig(document: ConfigDocument): Promise<ValidationResult> {
    return this.configApi.validate(document);
  }

  applyConfig(document: ConfigDocument, note: string): Promise<ApplyResult> {
    return this.configApi.apply(document, note);
  }

  listConfigVersions(): Promise<ConfigVersionSummary[]> {
    return this.configApi.versions();
  }

  verifyConfigVersion(version: number, note: string): Promise<ConfigVersionSummary> {
    return this.configApi.verifyVersion(version, note);
  }

  previewRollback(version: number): Promise<ValidationResult> {
    return this.configApi.previewRollback(version);
  }

  rollbackConfig(version: number, reason: string): Promise<ApplyResult> {
    return this.configApi.rollback(version, reason);
  }

  // Audit log, backup, saved files, settings

  getAuditEvents(query: AuditQuery): Promise<AuditPage> {
    return this.adminApi.auditEvents(query);
  }

  exportAudit(request: AuditExportRequest): Promise<AuditExportResult | null> {
    return this.adminApi.exportAudit(request);
  }

  verifyAudit(): Promise<AuditVerification> {
    return this.adminApi.verifyAudit();
  }

  saveReportFile(file: ReportFile): Promise<SavedReport | null> {
    return this.adminApi.saveReportFile(file);
  }

  createBackup(): Promise<BackupSummary> {
    return this.adminApi.createBackup();
  }

  getBackupStatus(): Promise<BackupStatus> {
    return this.adminApi.backupStatus();
  }

  getEngineSettings(): Promise<EngineSettings> {
    return this.adminApi.settings();
  }

  listSecrets(): Promise<SecretInfo[]> {
    return this.adminApi.secrets();
  }

  setSecret(name: string, value: string): Promise<void> {
    return this.adminApi.setSecret(name, value);
  }

  deleteSecret(name: string): Promise<void> {
    return this.adminApi.deleteSecret(name);
  }

  getEngineInfo(): Promise<EngineInfo> {
    return this.adminApi.info();
  }
}
