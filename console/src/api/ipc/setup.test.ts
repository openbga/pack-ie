import { describe, expect, it } from "vitest";
import { type BootstrapNotifications, notificationSettingsFrom } from "./setup";

describe("the bootstrap's notifications section", () => {
  it("is null when the bootstrap has none", () => {
    expect(notificationSettingsFrom(null)).toBeNull();
    expect(notificationSettingsFrom(undefined)).toBeNull();
  });

  it("has no targets when none is configured, with the default rate limit", () => {
    expect(notificationSettingsFrom({})).toEqual({
      rateLimitMinutes: 15,
      email: null,
      webhook: null,
    });
    expect(notificationSettingsFrom({ email: null, webhook: null })).toEqual({
      rateLimitMinutes: 15,
      email: null,
      webhook: null,
    });
  });

  it("fills in the defaults of a minimal e-mail relay and webhook", () => {
    const raw: BootstrapNotifications = {
      email: { host: "smtp.lab.test", port: 587, from: "packie@lab.test", to: ["a@lab.test"] },
      webhook: { url: "https://hooks.lab.test/x" },
    };
    expect(notificationSettingsFrom(raw)).toEqual({
      rateLimitMinutes: 15,
      email: {
        host: "smtp.lab.test",
        port: 587,
        security: "starttls",
        username: null,
        passwordSecret: null,
        from: "packie@lab.test",
        to: ["a@lab.test"],
        severities: ["warning", "critical"],
      },
      webhook: {
        url: "https://hooks.lab.test/x",
        bearerSecret: null,
        severities: ["warning", "critical"],
        timeoutSeconds: 10,
      },
    });
  });

  it("keeps what is set, and takes a secret's name from its reference", () => {
    const raw: BootstrapNotifications = {
      rate_limit_minutes: 60,
      email: {
        host: "localhost",
        port: 25,
        security: "none",
        username: "packie",
        password: { secret: "smtp-password" },
        from: "packie@lab.test",
        to: ["a@lab.test", "b@lab.test"],
        severities: ["critical"],
      },
      webhook: {
        url: "http://localhost:9000/hook",
        bearer: { secret: "hook-token" },
        severities: ["warning"],
        timeout_seconds: 30,
      },
    };
    expect(notificationSettingsFrom(raw)).toEqual({
      rateLimitMinutes: 60,
      email: {
        host: "localhost",
        port: 25,
        security: "none",
        username: "packie",
        passwordSecret: "smtp-password",
        from: "packie@lab.test",
        to: ["a@lab.test", "b@lab.test"],
        severities: ["critical"],
      },
      webhook: {
        url: "http://localhost:9000/hook",
        bearerSecret: "hook-token",
        severities: ["warning"],
        timeoutSeconds: 30,
      },
    });
  });

  it("treats a null secret reference as none", () => {
    const raw: BootstrapNotifications = {
      email: { host: "h", port: 25, from: "a@b", to: ["c@d"], username: null, password: null },
      webhook: { url: "https://h/x", bearer: null },
    };
    const settings = notificationSettingsFrom(raw);
    expect(settings?.email?.passwordSecret).toBeNull();
    expect(settings?.webhook?.bearerSecret).toBeNull();
  });
});
