import { describe, expect, it } from "vitest";
import type { AuditFilter } from "../types";
import { mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });
const tests = {
  filter: { action: "notification_test" } as AuditFilter,
  beforeSeq: null,
  limit: 10,
};

const failingAlert = async (api: ReturnType<typeof create>) =>
  (await api.listAlerts("open")).find((each) => each.kind === "notifications_failing");

describe("the mock's notification targets", () => {
  it("reports the bootstrap's section with secret names that exist in the secret store", async () => {
    const api = create();
    const { notifications } = await api.getEngineSettings();
    const secrets = new Set((await api.listSecrets()).map((each) => each.name));
    expect(notifications?.email?.passwordSecret).toBeTruthy();
    expect(notifications?.webhook?.bearerSecret).toBeTruthy();
    expect(secrets.has(notifications?.email?.passwordSecret ?? "")).toBe(true);
    expect(secrets.has(notifications?.webhook?.bearerSecret ?? "")).toBe(true);
    expect(notifications?.rateLimitMinutes).toBe(15);
  });

  it("answers a test with one result per target and raises no alert when all are sent", async () => {
    const api = create();
    expect(await api.testNotification()).toEqual([
      { target: "email", ok: true, detail: "sent" },
      { target: "webhook", ok: true, detail: "sent" },
    ]);
    expect(await failingAlert(api)).toBeUndefined();
  });

  it("raises notifications_failing at once for a failing target and names it with its error", async () => {
    const api = create();
    api.failNotificationTarget("webhook", "HTTP 503");
    const alert = await failingAlert(api);
    expect(alert?.severity).toBe("warning");
    expect(alert?.title).toBe("Notifications are failing: webhook (HTTP 503)");
    expect(alert?.detail).toMatch(/^targets: webhook, failing since: \d{4}-/);
    expect(alert?.channelId).toBeNull();
    expect(alert?.nextAction).toContain("then send a test message (`packie notify test`)");
    expect(alert?.nextAction).toContain(
      "clears after the next successful delivery to every target",
    );
  });

  it("makes the target's tests fail with the error, the other still sent; the default error is connection refused", async () => {
    const api = create();
    api.failNotificationTarget("webhook");
    expect(await api.testNotification()).toEqual([
      { target: "email", ok: true, detail: "sent" },
      { target: "webhook", ok: false, detail: "connection refused" },
    ]);
    expect((await failingAlert(api))?.title).toBe(
      "Notifications are failing: webhook (connection refused)",
    );
  });

  it("names every failing target in one alert and keeps when it first failed", async () => {
    const api = create();
    api.failNotificationTarget("email", "535 authentication failed");
    const first = await failingAlert(api);
    api.failNotificationTarget("webhook", "HTTP 503");
    const both = await failingAlert(api);
    expect(both?.id).toBe(first?.id);
    expect(both?.title).toBe(
      "Notifications are failing: email (535 authentication failed), webhook (HTTP 503)",
    );
    expect(both?.detail).toMatch(/^targets: email, webhook, /);
    expect(both?.firstSeenAt).toBe(first?.firstSeenAt);
  });

  it("clears the alert only when a later test succeeds for every failing target", async () => {
    const api = create();
    api.failNotificationTarget("email");
    api.failNotificationTarget("webhook");
    api.failNotificationTarget(null);
    // Repairing the targets is not a send: the alert stays until one succeeds.
    expect(await failingAlert(api)).toBeDefined();
    await api.testNotification();
    expect(await failingAlert(api)).toBeUndefined();
    const cleared = (await api.listAlerts("cleared")).find(
      (each) => each.kind === "notifications_failing",
    );
    expect(cleared?.state).toBe("cleared");
  });

  it("keeps the alert while one target still fails", async () => {
    const api = create();
    api.failNotificationTarget("email");
    api.failNotificationTarget("webhook", "HTTP 503");
    await api.testNotification();
    expect((await failingAlert(api))?.title).toBe(
      "Notifications are failing: email (connection refused), webhook (HTTP 503)",
    );
  });

  it("answers no results when no target is configured, and reports no section", async () => {
    const api = create();
    api.setNotificationsConfigured("none");
    expect(await api.testNotification()).toEqual([]);
    expect((await api.getEngineSettings()).notifications).toBeNull();
  });

  it("raises no alert for a target the engine does not have", async () => {
    const api = create();
    api.failNotificationTarget("webhook");
    api.setNotificationsConfigured("none");
    expect(await failingAlert(api)).toBeUndefined();
  });

  it("leaves the section out when the bootstrap is unreadable, though the engine has its targets", async () => {
    const api = create();
    api.setNotificationsConfigured("unknown");
    const settings = await api.getEngineSettings();
    expect("notifications" in settings).toBe(false);
    expect((await api.testNotification()).map((each) => each.target)).toEqual(["email", "webhook"]);
    api.setNotificationsConfigured("both");
    expect((await api.getEngineSettings()).notifications?.email).not.toBeNull();
  });
});

describe("a test message in the audit log", () => {
  it("is recorded as requested, then with each target's result on its outcome", async () => {
    const api = create();
    api.failNotificationTarget("webhook");
    await api.testNotification();
    const [outcome, requested] = (await api.getAuditEvents(tests)).events;
    expect([requested?.outcome, outcome?.outcome]).toEqual(["requested", "succeeded"]);
    expect(requested?.details).toEqual({});
    expect(outcome?.details).toEqual({
      results: [
        { target: "email", ok: true },
        { target: "webhook", ok: false },
      ],
    });
    expect(outcome?.objectType).toBeNull();
    expect(JSON.stringify(outcome?.details)).not.toContain("connection refused");
  });

  it("is recorded with an empty result when there is no target", async () => {
    const api = create();
    api.setNotificationsConfigured("none");
    await api.testNotification();
    expect((await api.getAuditEvents(tests)).events[0]?.details).toEqual({ results: [] });
  });

  it("is refused, and nothing is sent, while the audit log cannot be written", async () => {
    const api = create();
    api.failNotificationTarget("webhook");
    api.setAuditAvailable(false);
    const error = await rejection(api.testNotification());
    expect(error.code).toBe("unavailable");
    api.setAuditAvailable(true);
    expect((await api.getAuditEvents(tests)).events).toEqual([]);
    // Nothing was sent, so nothing was learned about the targets either way.
    expect(await failingAlert(api)).toBeDefined();
    api.failNotificationTarget(null);
    api.setAuditAvailable(false);
    await rejection(api.testNotification());
    expect(await failingAlert(api)).toBeDefined();
  });
});
