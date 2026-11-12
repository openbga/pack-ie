import type {
  ConfigDocument,
  EngineInfo,
  EngineSettings,
  NotificationSettings,
  SecretInfo,
} from "../types";

interface StoredSecret {
  storage: SecretInfo["storage"];
  hasValue: boolean;
}

/** Secrets by name: the value is never stored or readable, only whether one is set. */
export class SecretStore {
  private readonly secrets = new Map<string, StoredSecret>([
    ["orders-sftp-key", { storage: "os_keystore", hasValue: true }],
    ["reflab-sftp-password", { storage: "os_keystore", hasValue: true }],
    ["billing-sftp-password", { storage: "os_keystore", hasValue: true }],
    ["clinic-sftp-key", { storage: "restricted_file", hasValue: false }],
    ["dashboard-api-token", { storage: "restricted_file", hasValue: true }],
    ["alert-smtp-password", { storage: "os_keystore", hasValue: true }],
    ["alert-webhook-token", { storage: "os_keystore", hasValue: true }],
  ]);

  names(): Set<string> {
    return new Set(this.secrets.keys());
  }

  has(name: string): boolean {
    return this.secrets.has(name);
  }

  set(name: string): void {
    const existing = this.secrets.get(name);
    this.secrets.set(name, { storage: existing?.storage ?? "os_keystore", hasValue: true });
  }

  delete(name: string): void {
    this.secrets.delete(name);
  }

  list(document: ConfigDocument): SecretInfo[] {
    return [...this.secrets.entries()]
      .map(([name, secret]) => ({
        name,
        storage: secret.storage,
        hasValue: secret.hasValue,
        usedByEndpointIds: document.endpoints
          .filter((endpoint) => endpoint.kind === "sftp" && endpoint.secretName === name)
          .map((endpoint) => endpoint.id),
      }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }
}

/** The bootstrap's `notifications` section as the mock reads it (the webhook path stands in for a token). */
export const MOCK_NOTIFICATIONS: NotificationSettings = {
  rateLimitMinutes: 15,
  email: {
    host: "smtp.lab.example.test",
    port: 587,
    security: "starttls",
    username: "packie-alerts",
    passwordSecret: "alert-smtp-password",
    from: "packie@lab.example.test",
    to: ["lab-oncall@lab.example.test", "lab-manager@lab.example.test"],
    severities: ["warning", "critical"],
  },
  webhook: {
    url: "https://hooks.lab.example.test:8443/services/T0001/B0002/secret-token-0003",
    bearerSecret: "alert-webhook-token",
    severities: ["critical"],
    timeoutSeconds: 10,
  },
};

/**
 * `notifications` is the console's reading of the bootstrap: the section, null when it has
 * none, undefined when no bootstrap could be read.
 */
export function engineSettings(
  logRingSize: number,
  notifications: NotificationSettings | null | undefined,
): EngineSettings {
  return {
    nodeId: "lab-pc-01",
    nodeName: "Main Lab Interface PC",
    stateDir: "C:\\ProgramData\\PACK-IE\\state",
    logDir: "C:\\ProgramData\\PACK-IE\\logs",
    ipcPath: "\\\\.\\pipe\\packie-lab-pc-01",
    bootstrapPath: "C:\\ProgramData\\PACK-IE\\bootstrap.json",
    hosting: "service",
    logRetentionDays: 30,
    logRingSize,
    ...(notifications === undefined ? {} : { notifications }),
  };
}

export function engineInfo(): EngineInfo {
  return {
    engineVersion: "0.2.0",
    consoleVersion: "0.2.0",
    protocolVersion: 1,
    buildCommit: "b24295f",
    platform: "windows-x86_64",
    license: "AGPL-3.0-only",
  };
}
