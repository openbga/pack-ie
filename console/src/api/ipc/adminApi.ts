/**
 * What an operator does to the installation around the channels: the audit log, backups,
 * files the shell saves, the engine's settings and secrets, and the version dialog.
 */

import type {
  AuditExportRequest,
  AuditExportResult,
  AuditPage,
  AuditQuery,
  AuditVerification,
  BackupStatus,
  BackupSummary,
  EngineInfo,
  EngineSettings,
  ReportFile,
  SavedReport,
  SecretInfo,
} from "../types";
import {
  auditExportToIpc,
  auditPageFromIpc,
  auditQueryToIpc,
  backupStatusFromIpc,
  backupSummaryFromIpc,
  verificationFromIpc,
} from "./audit";
import type { TauriBridge } from "./bridge";
import type { IpcClient } from "./client";
import type { ConfigCache } from "./configCache";
import { engineError, fromCommandError } from "./errors";
import { type BootstrapDocument, engineSettingsFrom, platformName, secretsFrom } from "./settings";

export class AdminApi {
  constructor(
    private readonly client: IpcClient,
    private readonly configs: ConfigCache,
    private readonly bridge: TauriBridge,
  ) {}

  // Audit log

  async auditEvents(query: AuditQuery): Promise<AuditPage> {
    const page = await this.client.request(
      { type: "get_audit_events", ...auditQueryToIpc(query) },
      "audit_events",
    );
    return auditPageFromIpc(page);
  }

  async exportAudit(request: AuditExportRequest): Promise<AuditExportResult | null> {
    let saved: { path: string; rows: number } | null;
    try {
      saved = await this.bridge.invoke("save_audit_export", {
        suggestedName: `packie-audit-${new Date().toISOString().slice(0, 10)}.${
          request.format === "csv" ? "csv" : "jsonl"
        }`,
        ...auditExportToIpc(request),
      });
    } catch (error) {
      throw fromCommandError(error);
    }
    return saved === null ? null : { savedTo: saved.path, rows: saved.rows };
  }

  async verifyAudit(): Promise<AuditVerification> {
    const result = await this.client.request({ type: "verify_audit" }, "audit_verification");
    return verificationFromIpc(result);
  }

  async saveReportFile(file: ReportFile): Promise<SavedReport | null> {
    // Recorded before anything is written: an audit log that cannot take it refuses the export.
    if (file.audit !== undefined) {
      await this.client.request(
        { type: "record_client_export", kind: file.audit.kind, rows: file.audit.rows },
        "done",
      );
    }
    let saved: { path: string } | null;
    try {
      saved = await this.bridge.invoke("save_report_file", {
        suggestedName: file.name,
        extension: file.format,
        content: file.content,
      });
    } catch (error) {
      throw fromCommandError(error);
    }
    return saved === null ? null : { savedTo: saved.path };
  }

  // Backup

  async createBackup(): Promise<BackupSummary> {
    const summary = await this.client.request(
      { type: "create_backup", directory: null },
      "backup_created",
    );
    return backupSummaryFromIpc(summary);
  }

  async backupStatus(): Promise<BackupStatus> {
    const status = await this.client.request({ type: "backup_status" }, "backup_status");
    return backupStatusFromIpc(status);
  }

  // Settings

  async settings(): Promise<EngineSettings> {
    let bootstrap: BootstrapDocument | null;
    try {
      bootstrap = await this.bridge.invoke<BootstrapDocument | null>("read_bootstrap");
    } catch (error) {
      throw fromCommandError(error);
    }
    const connection = await this.client.currentConnection().catch(() => null);
    const hosting = await this.client
      .request({ type: "get_snapshot" }, "snapshot")
      .then(({ snapshot }) => snapshot.node.hosting_mode)
      .catch(() => null);
    // Only the path is optional: the restore instructions say how to find it when it is not known.
    const bootstrapPath = await this.bridge
      .invoke<string | null>("bootstrap_file")
      .catch(() => null);
    return engineSettingsFrom(bootstrap, connection, hosting, bootstrapPath);
  }

  async secrets(): Promise<SecretInfo[]> {
    const [config, stored] = await Promise.all([
      this.configs.current(),
      this.client.request({ type: "list_secrets" }, "secrets"),
    ]);
    return secretsFrom(config, stored.names);
  }

  async setSecret(name: string, value: string): Promise<void> {
    await this.client.request({ type: "set_secret", name, value }, "done");
  }

  async deleteSecret(name: string): Promise<void> {
    await this.client.request({ type: "delete_secret", name }, "done");
  }

  async info(): Promise<EngineInfo> {
    const { engine } = await this.client.currentConnection();
    if (engine === null) throw engineError("engine_unavailable", "The engine is not running.");
    let consoleVersion: string;
    try {
      consoleVersion = await this.bridge.invoke<string>("app_version");
    } catch (error) {
      throw fromCommandError(error);
    }
    return {
      engineVersion: engine.engine_version,
      consoleVersion,
      protocolVersion: engine.protocol_version,
      buildCommit: null,
      platform: platformName(navigator.userAgent),
      license: "AGPL-3.0-only",
    };
  }
}
