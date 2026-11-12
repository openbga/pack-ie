import { screen } from "@testing-library/react";
import { vi } from "vitest";
import type { EngineInfo, EngineSettings, SecretInfo } from "@/api/types";
import { stubApi } from "@/features/config/testFixtures";
import { SettingsPage } from "./SettingsPage";

/** The settings page at `/settings/<tab>`, for `renderRoutes`. */
export const settingsRoutes = [{ path: "/settings/:tab", element: <SettingsPage /> }];

export const BOOTSTRAP = "/etc/packie/bootstrap.json";

/** What an engine running as a service reports about its bootstrap file. */
export function engineSettings(overrides: Partial<EngineSettings> = {}): EngineSettings {
  return {
    nodeId: "node-1",
    nodeName: "Lab computer",
    stateDir: "/var/lib/packie",
    logDir: "/var/log/packie",
    ipcPath: "/var/lib/packie/packie.sock",
    bootstrapPath: BOOTSTRAP,
    hosting: "service",
    logRetentionDays: 14,
    logRingSize: 20000,
    ...overrides,
  };
}

export function engineInfo(platform = "macos-aarch64"): EngineInfo {
  return {
    engineVersion: "0.1.0",
    consoleVersion: "0.1.0",
    protocolVersion: 1,
    buildCommit: "abc1234",
    platform,
    license: "AGPL-3.0-only",
  };
}

const SECRETS: SecretInfo[] = [
  { name: "lis-sftp", storage: "os_keystore", hasValue: true, usedByEndpointIds: ["lis-out"] },
  { name: "backup-sftp", storage: "restricted_file", hasValue: false, usedByEndpointIds: [] },
];

/** An engine with settings, build info and two secrets, for the Engine, Retention and Secrets tabs. */
export function settingsApi(overrides = {}) {
  return stubApi({
    getEngineSettings: async () => engineSettings(),
    getEngineInfo: async () => engineInfo(),
    listSecrets: vi.fn(async () => SECRETS),
    setSecret: vi.fn(async () => undefined),
    deleteSecret: vi.fn(async () => undefined),
    listConfigVersions: async () => [],
    ...overrides,
  });
}

/** The value beside a label of a facts list. */
export const fact = (label: string): HTMLElement => {
  const term = screen.getByText(label, { selector: "dt" });
  return term.nextElementSibling as HTMLElement;
};
