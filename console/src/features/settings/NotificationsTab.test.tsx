import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { NotificationSettings } from "@/api/types";
import { engineFailure } from "@/features/config/testFixtures";
import {
  CONFIGURED,
  notificationsApi,
  openNotifications,
  secret,
} from "./notificationsTestFixtures";
import { engineSettings, fact } from "./testFixtures";

describe("Settings > Notifications > targets", () => {
  it("lists both targets read-only, with secret names and whether each is stored", async () => {
    await openNotifications(notificationsApi(CONFIGURED));
    expect(await screen.findByText("Relay", { selector: "dt" })).toBeTruthy();
    expect(fact("Rate limit").textContent).toContain("15 minutes");
    expect(fact("Relay").textContent).toBe("smtp.lab.test:587");
    expect(fact("Security").textContent).toBe("STARTTLS (encrypted after connecting)");
    expect(fact("Sender").textContent).toBe("packie@lab.test");
    expect(fact("Recipients").textContent).toBe("oncall@lab.test, manager@lab.test");
    expect(fact("Login name").textContent).toBe("packie-alerts");
    await waitFor(() =>
      expect(within(fact("Password secret")).getByText("In the secret store")).toBeTruthy(),
    );
    expect(fact("Password secret").textContent).toContain("smtp-password");
    expect(fact("Bearer secret").textContent).toContain("webhook-token");
    expect(within(fact("Bearer secret")).getByText("In the secret store")).toBeTruthy();
    const severities = screen.getAllByText("Alert severities sent", { selector: "dt" });
    expect(severities.map((each) => each.nextElementSibling?.textContent)).toEqual([
      "Warning, Critical",
      "Critical",
    ]);
    expect(fact("Timeout").textContent).toBe("10 seconds");
    expect(screen.getByText(/changing them needs a restart/)).toBeTruthy();
    expect(screen.getByText(/edit the notifications section of bootstrap.json/)).toBeTruthy();
    expect(screen.queryByText(/not in the secret store/i)).toBeNull();
  });

  it("shows the webhook's host and port only, never its path or query", async () => {
    const { container } = await openNotifications(notificationsApi(CONFIGURED));
    expect(await screen.findByText("https://hooks.lab.test:8443")).toBeTruthy();
    expect(container.textContent).not.toContain("secret-token");
    expect(container.textContent).not.toContain("services");
    expect(container.textContent).not.toContain("key=abc");
  });

  it("shows nothing of an address it cannot read", async () => {
    const webhook = {
      url: "not a url /token",
      bearerSecret: null,
      severities: ["critical"],
      timeoutSeconds: 10,
    };
    const { container } = await openNotifications(notificationsApi({ ...CONFIGURED, webhook }));
    expect(await screen.findByText("The console cannot read the address")).toBeTruthy();
    expect(container.textContent).not.toContain("/token");
    expect(container.textContent).not.toContain("not a url");
    expect(fact("Bearer secret").textContent).toBe("None: no Authorization header is sent");
  });

  it("says plainly that a relay on this computer needs no encryption or login", async () => {
    await openNotifications(
      notificationsApi({
        ...CONFIGURED,
        email: {
          ...(CONFIGURED.email as NonNullable<NotificationSettings["email"]>),
          host: "localhost",
          port: 25,
          security: "none",
          username: null,
          passwordSecret: null,
        },
      }),
    );
    expect((await screen.findByText("Security", { selector: "dt" })).nextSibling?.textContent).toBe(
      "No encryption (this computer only)",
    );
    expect(fact("Login name").textContent).toBe("None: the relay is used without a login");
    expect(fact("Password secret").textContent).toBe("None");
  });

  it("flags a secret that is not in the store, or has no value, with a link to Secrets", async () => {
    const { user } = await openNotifications(
      notificationsApi(CONFIGURED, {
        listSecrets: vi.fn(async () => [secret("webhook-token", false)]),
      }),
    );
    await screen.findByText("Relay", { selector: "dt" });
    const password = fact("Password secret");
    await waitFor(() => expect(within(password).getByText("Missing")).toBeTruthy());
    expect(password.textContent).toContain(
      "Not in the secret store: a send fails until it is stored.",
    );
    const bearer = fact("Bearer secret");
    expect(within(bearer).getByText("No value")).toBeTruthy();
    expect(bearer.textContent).toContain("without a value: a send fails until one is set");
    const link = within(password).getByRole("link", { name: "Open Settings → Secrets" });
    expect(link.getAttribute("href")).toBe("/settings/secrets");
    await user.click(link);
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Secrets" }).getAttribute("aria-selected")).toBe(
        "true",
      ),
    );
  });

  it("does not guess when the secret store cannot be read", async () => {
    await openNotifications(
      notificationsApi(CONFIGURED, {
        listSecrets: vi.fn(async () => {
          throw engineFailure("internal", "The secret store failed.");
        }),
      }),
    );
    await waitFor(() =>
      expect(
        within(fact("Password secret")).getByText("Could not check the secret store."),
      ).toBeTruthy(),
    );
    expect(screen.queryByText("Missing")).toBeNull();
  });

  it("says a target that is not configured is not", async () => {
    await openNotifications(notificationsApi({ ...CONFIGURED, email: null }));
    expect(await screen.findByText("Not configured: no e-mail is sent.")).toBeTruthy();
    expect(screen.queryByText("Not configured: no webhook is called.")).toBeNull();
  });

  it("says notifications are off when the bootstrap has no section, and disables the test", async () => {
    await openNotifications(notificationsApi(null));
    expect(await screen.findByText("Notifications are off")).toBeTruthy();
    expect(screen.getByText(/No e-mail relay or webhook is configured/)).toBeTruthy();
    expect(screen.getByText(/notifications\.email or notifications\.webhook/)).toBeTruthy();
    const button = screen.getByRole("button", { name: "Send test…" });
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(
      screen.getByText(/Disabled: no target is configured in the bootstrap file/),
    ).toBeTruthy();
  });

  it("says the targets cannot be listed when the bootstrap is unknown, but still allows a test", async () => {
    const { user } = await openNotifications(notificationsApi(undefined));
    expect(await screen.findByText(/could not read the engine's bootstrap file/)).toBeTruthy();
    expect(screen.queryByText("Notifications are off")).toBeNull();
    const button = screen.getByRole("button", { name: "Send test…" });
    expect(button.hasAttribute("disabled")).toBe(false);
    await user.click(button);
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain("every notification target the engine has");
    expect(within(dialog).queryByRole("list")).toBeNull();
  });

  it("shows loading, then an error with a retry that works", async () => {
    const read = vi
      .fn()
      .mockRejectedValueOnce(engineFailure("engine_unavailable", "The engine is not running."))
      .mockResolvedValue(engineSettings({ notifications: CONFIGURED }));
    const { user } = await openNotifications(
      notificationsApi(CONFIGURED, { getEngineSettings: read }),
    );
    expect(await screen.findByText("The notification settings could not be loaded")).toBeTruthy();
    // The test can still be sent: the engine knows its own targets.
    expect(screen.getByRole("button", { name: "Send test…" }).hasAttribute("disabled")).toBe(false);
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Relay", { selector: "dt" })).toBeTruthy();
  });
});
