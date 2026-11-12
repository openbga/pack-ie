import { expect, type Locator, type Page, test } from "@playwright/test";
import { expectNoAxeViolations } from "./a11y.support";
import { COLUMN, openAudit } from "./audit.support";
import { dataRows, eventPanel, heading, type MockSwitches, mockHandle, open } from "./support";

test.use({ timezoneId: "UTC" });

/** What the mock's bootstrap says (src/api/mock/settings.ts `MOCK_NOTIFICATIONS`). */
const WEBHOOK_ORIGIN = "https://hooks.lab.example.test:8443";
const WEBHOOK_PATH = "/services/T0001/B0002/secret-token-0003";
const CONNECTION_REFUSED = "Notifications are failing: webhook (connection refused)";

/** The notification targets, as `window.__packieMock` lets a journey set them. */
type Mode = "both" | "none" | "unknown";

/** Opens Settings → Notifications; `mode` is set first, on the dashboard, because the tab reads the bootstrap once. */
async function openNotifications(
  page: Page,
  mode: Mode = "both",
  switches: MockSwitches = {},
): Promise<void> {
  if (mode === "both") {
    await open(page, "/settings/notifications", switches);
  } else {
    await open(page, "/dashboard", switches);
    await expect(heading(page, "Dashboard")).toBeVisible();
    // The function is sent to the page as text: it cannot use `mode`.
    if (mode === "none") await mockHandle(page, (mock) => mock.setNotificationsConfigured("none"));
    else await mockHandle(page, (mock) => mock.setNotificationsConfigured("unknown"));
    await goTo(page, "/settings/notifications");
  }
  await expect(heading(page, "Settings")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Notifications" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("heading", { level: 2, name: "Status" })).toBeVisible();
  await expect(page.getByText("Loading ")).toHaveCount(0);
}

/** Moves inside the app without reloading, so the mock engine keeps its state. */
async function goTo(page: Page, route: string): Promise<void> {
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, route);
}

function factTerm(page: Page, label: string): Locator {
  return page.getByRole("main").locator("dt", { hasText: new RegExp(`^${label}$`) });
}

/** The value beside `label`; `nth` picks between the e-mail and the webhook for shared labels. */
function fact(page: Page, label: string, nth = 0): Locator {
  return factTerm(page, label).nth(nth).locator("xpath=following-sibling::dd");
}

const result = (page: Page): Locator => page.getByRole("region", { name: "Test result" });
const status = (page: Page): Locator => page.getByRole("region", { name: "Status" });

/** Presses Send test… and confirms, as an operator does. */
async function sendTest(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Send test…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Send a test message?" });
  await dialog.getByRole("button", { name: "Send test message" }).click();
  await expect(dialog).toHaveCount(0);
}

test("NT-01 the tab shows both targets read-only with secret names and webhook origin only", async ({
  page,
}) => {
  await openNotifications(page);
  await expect(fact(page, "Rate limit")).toContainText("15 minutes");
  await expect(fact(page, "Relay")).toHaveText("smtp.lab.example.test:587");
  await expect(fact(page, "Security")).toHaveText("STARTTLS (encrypted after connecting)");
  await expect(fact(page, "Sender")).toHaveText("packie@lab.example.test");
  await expect(fact(page, "Recipients")).toHaveText(
    "lab-oncall@lab.example.test, lab-manager@lab.example.test",
  );
  await expect(fact(page, "Login name")).toHaveText("packie-alerts");
  await expect(fact(page, "Alert severities sent", 0)).toHaveText("Warning, Critical");
  await expect(fact(page, "Alert severities sent", 1)).toHaveText("Critical");
  await expect(fact(page, "Timeout")).toHaveText("10 seconds");

  // Only the NAME of a secret, and that it is in the store.
  await expect(fact(page, "Password secret")).toContainText("alert-smtp-password");
  await expect(fact(page, "Password secret")).toContainText("In the secret store");
  await expect(fact(page, "Bearer secret")).toContainText("alert-webhook-token");
  await expect(fact(page, "Bearer secret")).toContainText("In the secret store");

  // The address is shown as scheme, host and port: its path can carry a token.
  await expect(fact(page, "Address")).toContainText(WEBHOOK_ORIGIN);
  const text = (await page.getByRole("main").innerText()).replaceAll("\n", " ");
  expect(text).not.toContain(WEBHOOK_PATH);
  expect(text).not.toContain("secret-token-0003");
  expect(text).not.toContain("/services");

  // Nothing here edits them.
  await expect(page.getByRole("main").getByRole("textbox")).toHaveCount(0);
  await expect(page.getByText(/changing them needs a restart/)).toBeVisible();
  await expect(page.getByText(/edit the notifications section of bootstrap\.json/)).toBeVisible();
  await expect(status(page)).toContainText("No target is failing");
});

test("NT-01 a secret that is not in the store is flagged, with a way to Secrets", async ({
  page,
}) => {
  await open(page, "/settings/secrets");
  await expect(dataRows(page, "Secrets").filter({ hasText: "alert-webhook-token" })).toHaveCount(1);
  await page.getByRole("button", { name: "Actions for alert-webhook-token" }).click();
  await page.getByRole("menuitem", { name: "Delete secret…" }).click();
  await page
    .getByRole("alertdialog", { name: "Delete secret alert-webhook-token?" })
    .getByRole("button", { name: "Delete secret" })
    .click();
  await expect(page.getByText("Secret alert-webhook-token deleted")).toBeVisible();

  await page.getByRole("tab", { name: "Notifications" }).click();
  await expect(fact(page, "Bearer secret")).toContainText("alert-webhook-token");
  await expect(fact(page, "Bearer secret")).toContainText("Missing");
  await expect(fact(page, "Bearer secret")).toContainText(
    "Not in the secret store: a send fails until it is stored.",
  );
  await expect(fact(page, "Password secret")).toContainText("In the secret store");
  await fact(page, "Bearer secret").getByRole("link", { name: "Open Settings → Secrets" }).click();
  await expect(page.getByRole("tab", { name: "Secrets" })).toHaveAttribute("aria-selected", "true");
});

test("NT-02 Send test confirms first, then shows each target's answer", async ({ page }) => {
  await openNotifications(page);
  await page.getByRole("button", { name: "Send test…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Send a test message?" });
  await expect(dialog.getByRole("listitem")).toHaveText([
    "E-mail relay smtp.lab.example.test:587",
    `Webhook at ${WEBHOOK_ORIGIN}`,
  ]);
  await expect(dialog).toContainText("code test and the severity info");
  await expect(dialog).toContainText("Nothing from messages or channels is in it");
  await expect(dialog).toContainText(
    "recorded in the audit log with your account (notification_test)",
  );
  await expect(dialog).toContainText("A failed send is not retried");
  await expect(dialog).not.toContainText("secret-token-0003");

  // A cancel sends nothing.
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(result(page)).toHaveCount(0);

  await sendTest(page);
  await expect(result(page)).toBeVisible();
  await expect(result(page)).toContainText("2 of 2 targets accepted the test message");
  await expect(result(page).locator("dt")).toHaveText(["E-mail", "Webhook"]);
  await expect(result(page).locator("dd")).toHaveText(["Accepted sent", "Accepted sent"]);
  await expect(status(page)).toContainText("No target is failing");
});

test("NT-03 a failing webhook: the result, the status, the alert and its clearing", async ({
  page,
}) => {
  await openNotifications(page);
  await mockHandle(page, (mock) => mock.failNotificationTarget("webhook"));
  await sendTest(page);

  await expect(result(page)).toContainText("1 of 2 targets accepted the test message");
  await expect(result(page).locator("dd")).toHaveText([
    "Accepted sent",
    "Failed connection refused",
  ]);
  await expect(result(page)).toContainText("A failed send is not retried");

  // The status block follows the engine's alert.
  const failing = status(page).getByRole("alert");
  await expect(failing).toContainText(CONNECTION_REFUSED);
  await expect(failing).toContainText("First failed send:");
  await expect(failing).toContainText("then send a test message (`packie notify test`)");
  await expect(status(page)).not.toContainText("No target is failing");

  // The Alerts page lists it and links back to the Notifications settings.
  await failing.getByRole("link", { name: "Open Alerts" }).click();
  await expect(heading(page, "Alerts")).toBeVisible();
  const row = dataRows(page, "Alerts").filter({ hasText: CONNECTION_REFUSED });
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("then send a test message (`packie notify test`)");
  await row.getByRole("link", { name: "Notification settings" }).click();
  await expect(page.getByRole("tab", { name: "Notifications" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(status(page).getByRole("alert")).toContainText(CONNECTION_REFUSED);

  // Repairing the target is not enough: a later successful send clears the alert.
  await mockHandle(page, (mock) => mock.failNotificationTarget(null));
  await sendTest(page);
  await expect(result(page)).toContainText("2 of 2 targets accepted the test message");
  await expect(status(page)).toContainText("No target is failing");
  await expect(status(page).getByRole("alert")).toHaveCount(0);
  await goTo(page, "/alerts");
  await expect(heading(page, "Alerts")).toBeVisible();
  await expect(dataRows(page, "Alerts").first()).toBeVisible();
  await expect(dataRows(page, "Alerts").filter({ hasText: CONNECTION_REFUSED })).toHaveCount(0);
});

test("NT-03 the hook raises the alert at once, with the engine's error, on the Alerts page", async ({
  page,
}) => {
  await open(page, "/alerts");
  await expect(dataRows(page, "Alerts").first()).toBeVisible();
  await mockHandle(page, (mock) =>
    mock.failNotificationTarget("email", "535 authentication failed"),
  );
  await page.getByRole("button", { name: "Refresh" }).first().click();
  await expect(
    dataRows(page, "Alerts").filter({
      hasText: "Notifications are failing: email (535 authentication failed)",
    }),
  ).toHaveCount(1);
});

test("NT-04 no targets configured: notifications are off, and Send test cannot be pressed", async ({
  page,
}) => {
  await openNotifications(page, "none");
  await expect(page.getByText("Notifications are off")).toBeVisible();
  await expect(page.getByText(/No e-mail relay or webhook is configured/)).toBeVisible();
  await expect(page.getByText(/notifications\.email or notifications\.webhook/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Send test…" })).toBeDisabled();
  await expect(page.getByText(/Disabled: no target is configured/)).toBeVisible();
  await expect(factTerm(page, "Relay")).toHaveCount(0);
});

test("NT-04 the engine's answer wins: it has no target although the console listed some", async ({
  page,
}) => {
  await openNotifications(page);
  await expect(fact(page, "Relay")).toBeVisible();
  await mockHandle(page, (mock) => mock.setNotificationsConfigured("none"));
  await sendTest(page);
  await expect(result(page)).toContainText("No notification target is configured on the engine.");
  await expect(result(page).locator("dt")).toHaveCount(0);
});

test("NT-05 the bootstrap cannot be read: no targets are listed, a test can still be sent", async ({
  page,
}) => {
  await openNotifications(page, "unknown");
  await expect(page.getByText(/could not read the engine's bootstrap file/)).toBeVisible();
  await expect(page.getByText("Notifications are off")).toHaveCount(0);
  await expect(factTerm(page, "Relay")).toHaveCount(0);

  await page.getByRole("button", { name: "Send test…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Send a test message?" });
  await expect(dialog).toContainText("every notification target the engine has");
  await expect(dialog.getByRole("listitem")).toHaveCount(0);
  await dialog.getByRole("button", { name: "Send test message" }).click();
  await expect(result(page)).toContainText("2 of 2 targets accepted the test message");
  await expect(result(page).locator("dt")).toHaveText(["E-mail", "Webhook"]);
});

test("NT-06 the test is recorded in the audit log, with each target's result", async ({ page }) => {
  await openNotifications(page);
  await mockHandle(page, (mock) => mock.failNotificationTarget("webhook"));
  await sendTest(page);
  await expect(result(page)).toBeVisible();

  await goTo(page, "/audit?action=notification_test");
  await expect(heading(page, "Audit")).toBeVisible();
  const [outcome, requested] = [
    dataRows(page, "Audit events").nth(0),
    dataRows(page, "Audit events").nth(1),
  ];
  await expect(outcome.getByRole("gridcell").nth(COLUMN.action)).toHaveText(
    "Notification test sent",
  );
  await expect(outcome.getByRole("gridcell").nth(COLUMN.outcome)).toContainText("Succeeded");
  await expect(outcome.getByRole("gridcell").nth(COLUMN.actor)).toHaveText("LAB\\jdoe");
  await expect(requested.getByRole("gridcell").nth(COLUMN.outcome)).toContainText("Requested");
  await outcome.click();
  await expect(eventPanel(page)).toContainText("notification_test");
  await expect(eventPanel(page)).toContainText("results");
  await expect(eventPanel(page)).not.toContainText("connection refused");
});

test("NT-06 with the audit log unavailable the test is refused, in words, and nothing is sent", async ({
  page,
}) => {
  await openNotifications(page);
  await mockHandle(page, (mock) => mock.setAuditAvailable(false));
  await sendTest(page);
  const refused = page.getByRole("alert").filter({ hasText: "The test message was not sent" });
  await expect(refused).toContainText(
    "Not sent: the audit log cannot be written, and test messages are audited.",
  );
  await expect(result(page)).toHaveCount(0);

  // Once the log can be written, Try again sends it.
  await mockHandle(page, (mock) => mock.setAuditAvailable(true));
  await refused.getByRole("button", { name: "Try again" }).click();
  await expect(result(page)).toContainText("2 of 2 targets accepted the test message");
  await expect(refused).toHaveCount(0);

  await openAudit(page, "?action=notification_test");
  await expect(dataRows(page, "Audit events")).toHaveCount(2);
});

test.describe("accessibility", () => {
  test("NT-07 the tab, the confirmation, a failing status and the result have no axe violations", async ({
    page,
  }) => {
    await openNotifications(page);
    await expectNoAxeViolations(page);
    await mockHandle(page, (mock) => mock.failNotificationTarget("webhook"));
    await page.getByRole("button", { name: "Send test…" }).click();
    await expect(page.getByRole("alertdialog", { name: "Send a test message?" })).toBeVisible();
    await expectNoAxeViolations(page);
    await page.getByRole("button", { name: "Send test message" }).click();
    await expect(result(page)).toBeVisible();
    await expect(status(page).getByRole("alert")).toContainText(CONNECTION_REFUSED);
    await expectNoAxeViolations(page);
  });

  test("NT-07 the off state has no axe violations", async ({ page }) => {
    await openNotifications(page, "none");
    await expect(page.getByText("Notifications are off")).toBeVisible();
    await expectNoAxeViolations(page);
  });
});
