import { screen, within } from "@testing-library/react";
import { vi } from "vitest";
import type { Alert, NotificationResult, NotificationSettings, SecretInfo } from "@/api/types";
import { stubApi } from "@/features/config/testFixtures";
import { renderRoutes } from "@/lib/test/router";
import { engineSettings, settingsRoutes } from "./testFixtures";

export const CONFIGURED: NotificationSettings = {
  rateLimitMinutes: 15,
  email: {
    host: "smtp.lab.test",
    port: 587,
    security: "starttls",
    username: "packie-alerts",
    passwordSecret: "smtp-password",
    from: "packie@lab.test",
    to: ["oncall@lab.test", "manager@lab.test"],
    severities: ["warning", "critical"],
  },
  webhook: {
    url: "https://hooks.lab.test:8443/services/T0001/secret-token-0003?key=abc",
    bearerSecret: "webhook-token",
    severities: ["critical"],
    timeoutSeconds: 10,
  },
};

export function secret(name: string, hasValue: boolean | null = true): SecretInfo {
  return { name, storage: "os_keystore", hasValue, usedByEndpointIds: [] };
}

const STORED = [secret("smtp-password"), secret("webhook-token")];

export function notificationsApi(
  notifications?: NotificationSettings | null,
  overrides: Parameters<typeof stubApi>[0] = {},
) {
  return stubApi({
    getEngineSettings: vi.fn(async () => engineSettings({ notifications })),
    listSecrets: vi.fn(async () => STORED),
    listAlerts: vi.fn(async () => [] as Alert[]),
    testNotification: vi.fn(async () => [] as NotificationResult[]),
    ...overrides,
  });
}

export async function openNotifications(api: ReturnType<typeof notificationsApi>) {
  const view = renderRoutes(settingsRoutes, "/settings/notifications", { api });
  await screen.findByText("Status");
  return view;
}

/** Sends a test message from the loaded page, confirming the dialog. */
export async function sendTest(user: ReturnType<typeof renderRoutes>["user"]) {
  await screen.findByText("Relay", { selector: "dt" });
  await user.click(screen.getByRole("button", { name: "Send test…" }));
  const dialog = await screen.findByRole("alertdialog", { name: "Send a test message?" });
  await user.click(within(dialog).getByRole("button", { name: "Send test message" }));
}
