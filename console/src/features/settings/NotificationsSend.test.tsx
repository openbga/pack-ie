import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Alert, NotificationResult } from "@/api/types";
import { engineFailure } from "@/features/config/testFixtures";
import { alert } from "@/features/shared-operate/testFixtures";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import {
  CONFIGURED,
  notificationsApi,
  openNotifications,
  sendTest,
} from "./notificationsTestFixtures";

const failingAlert: Alert = alert({
  id: "al-nt",
  severity: "warning",
  kind: "notifications_failing",
  title: "Notifications are failing: webhook (connection refused)",
  detail: "targets: webhook, failing since: 2026-11-07T09:00:00Z",
  channelId: null,
  endpointId: null,
  firstSeenAt: "2026-11-07T09:00:00Z",
  nextAction:
    "Check the notification settings (host, port, security, credentials secret, webhook address) and that this computer can reach the target, then send a test message (`packie notify test`). The alert clears after the next successful delivery to every target.",
});

describe("Settings > Notifications > status", () => {
  it("says no target is failing when the alert is not open", async () => {
    await openNotifications(notificationsApi(CONFIGURED));
    expect(await screen.findByText("No target is failing")).toBeTruthy();
    expect(screen.queryByText(/latest send to each target succeeded/)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("ignores other open alerts", async () => {
    await openNotifications(
      notificationsApi(CONFIGURED, { listAlerts: vi.fn(async () => [alert()]) }),
    );
    expect(await screen.findByText("No target is failing")).toBeTruthy();
  });

  it("names the failing targets, the first failure, the next step and links to Alerts", async () => {
    const { user } = await openNotifications(
      notificationsApi(CONFIGURED, { listAlerts: vi.fn(async () => [failingAlert]) }),
    );
    const warning = await screen.findByRole("alert");
    expect(warning.textContent).toContain(
      "Notifications are failing: webhook (connection refused)",
    );
    expect(warning.textContent).toContain("targets: webhook, failing since: 2026-11-07T09:00:00Z");
    expect(warning.textContent).toContain("First failed send:");
    expect(within(warning).getByText(/Check the notification settings/)).toBeTruthy();
    expect(within(warning).getByText(/then send a test message/)).toBeTruthy();
    expect(screen.queryByText("No target is failing")).toBeNull();
    await user.click(within(warning).getByRole("link", { name: "Open Alerts" }));
    expect(await screen.findByText("Navigated to /alerts")).toBeTruthy();
  });

  it("shows an error with a retry when the alerts cannot be read", async () => {
    await openNotifications(
      notificationsApi(CONFIGURED, {
        listAlerts: vi.fn(async () => {
          throw engineFailure("engine_unavailable", "The engine is not running.");
        }),
      }),
    );
    expect(await screen.findByText("The notification status could not be loaded")).toBeTruthy();
  });

  it("is read again after a test", async () => {
    const listAlerts = vi.fn(async () => [] as Alert[]);
    const { user } = await openNotifications(
      notificationsApi(CONFIGURED, {
        listAlerts,
        testNotification: vi.fn(async () => [{ target: "webhook", ok: true, detail: "sent" }]),
      }),
    );
    await screen.findByText("No target is failing");
    const before = listAlerts.mock.calls.length;
    await sendTest(user);
    await screen.findByRole("region", { name: "Test result" });
    await waitFor(() => expect(listAlerts.mock.calls.length).toBeGreaterThan(before));
  });
});

describe("Settings > Notifications > Send test", () => {
  it("asks first: lists the targets and what is sent, and does nothing when cancelled", async () => {
    const api = notificationsApi(CONFIGURED);
    const { user } = await openNotifications(api);
    await screen.findByText("Relay", { selector: "dt" });
    await user.click(screen.getByRole("button", { name: "Send test…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Send a test message?" });
    const targets = within(dialog)
      .getAllByRole("listitem")
      .map((each) => each.textContent);
    expect(targets).toEqual([
      "E-mail relay smtp.lab.test:587",
      "Webhook at https://hooks.lab.test:8443",
    ]);
    expect(dialog.textContent).toContain("code test and the severity info");
    expect(dialog.textContent).toContain("Nothing from messages or channels is in it");
    expect(dialog.textContent).toContain(
      "recorded in the audit log with your account (notification_test)",
    );
    expect(dialog.textContent).toContain("A failed send is not retried");
    expect(dialog.textContent).not.toContain("secret-token");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(api.testNotification).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Test result" })).toBeNull();
  });

  it("shows progress, then each target's answer and a summary", async () => {
    let finish: (value: NotificationResult[]) => void = () => undefined;
    const api = notificationsApi(CONFIGURED, {
      testNotification: vi.fn(
        () => new Promise<NotificationResult[]>((resolve) => (finish = resolve)),
      ),
    });
    const { user } = await openNotifications(api);
    await sendTest(user);
    expect(await screen.findByText("Sending the test message…")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Test result" })).toBeNull();
    finish([
      { target: "email", ok: true, detail: "sent" },
      { target: "webhook", ok: true, detail: "sent" },
    ]);
    const result = await screen.findByRole("region", { name: "Test result" });
    expect(within(result).getByText("2 of 2 targets accepted the test message")).toBeTruthy();
    expect(within(result).getAllByText("Accepted")).toHaveLength(2);
    const labels = within(result)
      .getAllByRole("term")
      .map((each) => each.textContent);
    expect(labels).toEqual(["E-mail", "Webhook"]);
    expect(result.textContent).toContain("Accepted sent");
    expect(screen.queryByText("Sending the test message…")).toBeNull();
    expect(api.testNotification).toHaveBeenCalledTimes(1);
  });

  it("shows a failure beside a success, with the engine's detail, in words and not by colour alone", async () => {
    const api = notificationsApi(CONFIGURED, {
      testNotification: vi.fn(async () => [
        { target: "email", ok: true, detail: "sent" },
        { target: "webhook", ok: false, detail: "connection refused" },
      ]),
    });
    const { user } = await openNotifications(api);
    await sendTest(user);
    const result = await screen.findByRole("region", { name: "Test result" });
    expect(within(result).getByText("1 of 2 targets accepted the test message")).toBeTruthy();
    expect(within(result).getByText("Failed")).toBeTruthy();
    expect(within(result).getByText("Accepted")).toBeTruthy();
    expect(result.textContent).toContain("Failed connection refused");
    expect(result.textContent).toContain("A failed send is not retried");
  });

  it("says so when a single target is all there is", async () => {
    const api = notificationsApi(CONFIGURED, {
      testNotification: vi.fn(async () => [{ target: "webhook", ok: false, detail: "timed out" }]),
    });
    const { user } = await openNotifications(api);
    await sendTest(user);
    expect(await screen.findByText("0 of 1 target accepted the test message")).toBeTruthy();
  });

  it("keeps the result until the next test", async () => {
    const testNotification = vi
      .fn()
      .mockResolvedValueOnce([{ target: "webhook", ok: false, detail: "connection refused" }])
      .mockResolvedValue([{ target: "webhook", ok: true, detail: "sent" }]);
    const { user } = await openNotifications(notificationsApi(CONFIGURED, { testNotification }));
    await sendTest(user);
    expect(await screen.findByText("connection refused", { exact: false })).toBeTruthy();
    await sendTest(user);
    expect(await screen.findByText("1 of 1 target accepted the test message")).toBeTruthy();
    expect(screen.queryByText("connection refused", { exact: false })).toBeNull();
  });

  it("trusts the engine's answer when it has no target, even though the bootstrap lists some", async () => {
    const { user } = await openNotifications(notificationsApi(CONFIGURED));
    await sendTest(user);
    const result = await screen.findByRole("region", { name: "Test result" });
    expect(result.textContent).toContain("No notification target is configured on the engine.");
  });

  it("explains a refusal while the audit log cannot be written, and retries", async () => {
    const testNotification = vi
      .fn()
      .mockRejectedValueOnce(engineFailure("unavailable", "The audit log cannot be written."))
      .mockResolvedValue([{ target: "email", ok: true, detail: "sent" }]);
    const { user } = await openNotifications(notificationsApi(CONFIGURED, { testNotification }));
    await sendTest(user);
    expect(await screen.findByText("The test message was not sent")).toBeTruthy();
    expect(
      screen.getByText(
        /Not sent: the audit log cannot be written, and test messages are audited\./,
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Test result" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("region", { name: "Test result" })).toBeTruthy();
    expect(testNotification).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("The test message was not sent")).toBeNull();
  });

  it("shows any other failure in the engine's words with its reference", async () => {
    const testNotification = vi.fn(async () => {
      throw engineFailure("internal", "The engine could not start the test.");
    });
    const { user } = await openNotifications(notificationsApi(CONFIGURED, { testNotification }));
    await sendTest(user);
    expect(await screen.findByText("The test could not be run")).toBeTruthy();
    expect(screen.getByText("The engine could not start the test.")).toBeTruthy();
    expect(screen.getByText("test-ref")).toBeTruthy();
  });
});

describe("Settings > Notifications accessibility", () => {
  it("has no violations with targets, a failing alert and a result", async () => {
    const api = notificationsApi(CONFIGURED, {
      listAlerts: vi.fn(async () => [failingAlert]),
      testNotification: vi.fn(async () => [
        { target: "email", ok: true, detail: "sent" },
        { target: "webhook", ok: false, detail: "connection refused" },
      ]),
    });
    const { user, container } = await openNotifications(api);
    await sendTest(user);
    await screen.findByRole("region", { name: "Test result" });
    await expectNoA11yViolations(container);
  });

  it("has no violations when notifications are off", async () => {
    const { container } = await openNotifications(notificationsApi(null));
    await screen.findByText("Notifications are off");
    await expectNoA11yViolations(container);
  });
});
