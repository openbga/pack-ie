import { expect, type Page, test } from "@playwright/test";
import { expectNoAxeViolations } from "./a11y.support";
import { parseCsv } from "./download.support";
import { CHANNELS, toast } from "./operate.support";
import { choose, dataRows, eventPanel, heading, loaded, main, mockHandle, param } from "./support";
import {
  BUSY,
  COLUMN,
  column,
  countText,
  exportCsv,
  GRID,
  listedCount,
  openReport,
  readRows,
  runButton,
  runReport,
  STA,
  totals,
  totalsTable,
} from "./transmission.support";

test.use({ timezoneId: "UTC" });

const channelBox = (page: Page) => page.getByRole("combobox", { name: /Channel/ });
const mainNav = (page: Page) => page.getByRole("navigation", { name: "Main" });

test.describe("Reaching the report", () => {
  test("TR-01 a message channel's page links to it with the channel chosen, and Diagnose offers it too", async ({
    page,
  }) => {
    await openReport(page, "");
    // From the navigation: no channel is chosen yet, and the report is not run.
    await expect(channelBox(page)).toHaveValue("");
    await expect(runButton(page)).toBeDisabled();
    await expect(page.getByText("No report yet")).toBeVisible();
    await expect(
      page.getByText("Running a report is recorded in the audit log with your account."),
    ).toBeVisible();

    await page.goto(`/?mock=1&e2e=1&steady=1#/channels/${STA.id}`);
    await loaded(page, STA.label);
    await main(page).getByRole("link", { name: "Transmission report…" }).click();
    await loaded(page, "Transmission report");
    expect(param(page, "channel")).toBe(STA.id);
    await expect(channelBox(page)).toHaveValue(new RegExp(`^${STA.label}`));
    await expect(runButton(page)).toBeEnabled();
    await expect(page.getByText("No report yet")).toBeVisible();

    // From the navigation of another page: the Diagnose group names it.
    await mainNav(page).getByRole("link", { name: "Audit" }).click();
    await loaded(page, "Audit");
    await mainNav(page).getByRole("link", { name: "Transmission report" }).click();
    await loaded(page, "Transmission report");
    await expect(channelBox(page)).toHaveValue("");
  });

  test("TR-01 a bridge channel has no report", async ({ page }) => {
    await page.goto(`/?mock=1&e2e=1&steady=1#/channels/${CHANNELS.c503.id}`);
    await loaded(page, CHANNELS.c503.label);
    await expect(main(page).getByRole("link", { name: "Transmission report…" })).toHaveCount(0);
    await page.goto(`/?mock=1&e2e=1&steady=1#/transmission`);
    await loaded(page, "Transmission report");
    await channelBox(page).click();
    await expect(page.getByRole("option", { name: new RegExp(STA.label) })).toBeVisible();
    await expect(page.getByRole("option", { name: new RegExp(CHANNELS.c503.label) })).toHaveCount(
      0,
    );
  });
});

test.describe("Running the report", () => {
  test("TR-02 shows the totals of each destination, and the grid holds exactly their deliveries", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await runReport(page);
    await expect(heading(page, "Transmission report")).toBeVisible();

    const summary = main(page).getByRole("region", { name: "Report" });
    await expect(summary).toContainText(STA.label);
    await expect(summary).toContainText("Messages received");

    // The totals: every destination, each adding up across what was proven and how it ended.
    await expect(totalsTable(page).getByRole("columnheader")).toHaveText([
      "Destination",
      "Deliveries",
      "Accepted",
      "Received",
      "Written, not confirmed",
      "Unresolved",
      "Closed",
      "Pending",
    ]);
    const perDestination = await totals(page);
    expect(perDestination.length).toBeGreaterThan(1);
    for (const { counts } of perDestination) {
      const [deliveries = 0, ...parts] = counts;
      expect(deliveries).toBeGreaterThan(0);
      expect(parts.reduce((sum, part) => sum + part, 0)).toBe(deliveries);
    }
    const deliveries = perDestination.reduce((sum, each) => sum + (each.counts[0] ?? 0), 0);

    // The grid: as many rows as the destinations delivered, with the columns that matter.
    await expect(page.getByRole("grid", { name: GRID }).getByRole("columnheader")).toHaveText([
      "Received",
      "Message",
      "Control ID",
      "Content hash",
      "Destination",
      "Ack level",
      "Ack code",
      "ACK ID",
      "Acknowledged at",
      "Outcome",
    ]);
    expect(await listedCount(page)).toBe(deliveries);
    const rows = await readRows(page);
    expect(rows).toHaveLength(deliveries);
    expect(new Set(column(rows, "destination"))).toEqual(
      new Set(perDestination.map((each) => each.label)),
    );
    for (const row of rows) {
      expect(row[0]).toMatch(/^\d\d-\d\d \d\d:\d\d:\d\d$/);
      expect(row[1]).toMatch(/^msg-\d+$/);
    }
    expect(column(rows, "contentHash").every((hash) => /^[0-9a-f]{12}…$/.test(hash))).toBe(true);
    expect(column(rows, "control").some((control) => control !== "Not provided")).toBe(true);
    // What was proven is worded as proven: never "delivered" for a level below accepted.
    const levels = new Set(column(rows, "ackLevel"));
    for (const level of levels) {
      expect(["Accepted", "Received", "Written, not confirmed", "None"]).toContain(level);
    }
    expect(levels.has("Accepted")).toBe(true);
    expect(column(rows, "ackCode").some((code) => /^[A-Z]{2,3}$/.test(code))).toBe(true);
    // The acknowledgement's own control id is listed beside its code, where the reply had one.
    expect(column(rows, "ackControlId").some((id) => /^ACK[0-9A-Z]+$/.test(id))).toBe(true);
    for (const row of rows) {
      expect(row.join(" ")).not.toMatch(/\bdelivered\b.*\bAccepted\b/);
      if (row[COLUMN.ackLevel] === "None") expect(row[COLUMN.outcome]).not.toBe("Acknowledged");
    }

    // A message opens from its row.
    const first = dataRows(page, GRID).first().getByRole("link");
    await expect(first).toHaveAttribute("href", /^#?\/messages\/msg-\d+$/);
  });

  test("TR-02 the period is chosen first: seven days hold more than the last day", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await runReport(page);
    const day = await listedCount(page);
    await choose(page, "Period", "Last 7 days");
    expect(param(page, "range")).toBe("7d");
    // Changing the form does not run it.
    expect(await listedCount(page)).toBe(day);
    await runReport(page);
    await expect.poll(() => listedCount(page)).toBeGreaterThan(day);
  });

  test("TR-03 the destination filter narrows the grid but not the totals, and the CSV holds exactly the rows shown", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await runReport(page);
    const before = await totals(page);
    const all = await listedCount(page);
    const target = before[1]?.label ?? "";
    expect(target).not.toBe("");

    await choose(page, "Destination", target);
    await expect.poll(() => param(page, "destination")).not.toBeNull();
    const narrowed = before[1]?.counts[0] ?? 0;
    await expect.poll(() => listedCount(page)).toBe(narrowed);
    await expect(countText(page)).toHaveText(`${narrowed} of ${all} deliveries`);
    expect(narrowed).toBeLessThan(all);
    // The totals still list every destination, with the same numbers.
    expect(await totals(page)).toEqual(before);
    const shown = await readRows(page);
    expect(shown).toHaveLength(narrowed);
    expect(new Set(column(shown, "destination"))).toEqual(new Set([target]));

    const file = await exportCsv(page);
    expect(file.name).toMatch(
      new RegExp(`^packie-transmission-${STA.id}-\\d{4}-\\d\\d-\\d\\d\\.csv$`),
    );
    const [header, ...lines] = parseCsv(file.text);
    expect(header).toEqual([
      "channel",
      "period_since",
      "period_until",
      "generated_at",
      "message_id",
      "received_at",
      "source_endpoint_id",
      "source_peer",
      "config_version",
      "mapping_version",
      "message_state",
      "control_id",
      "content_hash",
      "destination_id",
      "destination",
      "delivery_state",
      "ack_level",
      "ack_code",
      "ack_control_id",
      "sent_hash",
      "attempts",
      "first_attempt_at",
      "acknowledged_at",
    ]);
    expect(lines).toHaveLength(narrowed);
    const at = (name: string) => header?.indexOf(name) ?? -1;
    expect(new Set(lines.map((line) => line[at("channel")]))).toEqual(new Set([STA.id]));
    expect(new Set(lines.map((line) => line[at("destination")]))).toEqual(new Set([target]));
    // The same deliveries as the grid, message by message, and both hashes in full.
    expect(lines.map((line) => line[at("message_id")]).sort()).toEqual(
      column(shown, "message").sort(),
    );
    for (const line of lines) {
      expect(line[at("content_hash")]).toMatch(/^[0-9a-f]{64}$/);
      expect(line[at("sent_hash")]).toMatch(/^([0-9a-f]{64})?$/);
    }
    expect(lines.some((line) => line[at("sent_hash")] !== "")).toBe(true);
    // Where it came from and which configuration it ran under, for the lab's validation record.
    for (const line of lines) {
      expect(line[at("source_endpoint_id")]).toMatch(/^ep-/);
      expect(line[at("config_version")]).toMatch(/^\d+$/);
      expect(line[at("mapping_version")]).toMatch(/^(\d+)?$/);
    }
    expect(lines.some((line) => (line[at("source_peer")] ?? "") !== "")).toBe(true);
    expect(lines.some((line) => /^[A-Z]{2,3}$/.test(line[at("ack_code")] ?? ""))).toBe(true);
    expect(lines.some((line) => /^ACK[0-9A-Z]+$/.test(line[at("ack_control_id")] ?? ""))).toBe(
      true,
    );
    // The dialog said what the file holds, and the success notice names the file.
    await expect(toast(page, "Transmission report exported")).toContainText(file.name);

    // With every destination shown again the file holds every delivery.
    await choose(page, "Destination", "All destinations");
    await expect.poll(() => listedCount(page)).toBe(all);
    const everything = await exportCsv(page);
    expect(parseCsv(everything.text)).toHaveLength(all + 1);
  });

  test("TR-03 the export dialog says the file lists control ids and hashes, not message content", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await runReport(page);
    await page.getByRole("button", { name: "Export CSV…" }).click();
    const dialog = page.getByRole("dialog", { name: "Export transmission report" });
    await expect(dialog).toContainText("control id");
    await expect(dialog).toContainText("hashes");
    await expect(dialog).toContainText("no message content");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
  });

  test("TR-04 a period with no messages says so, and offers neither totals nor export", async ({
    page,
  }) => {
    await openReport(
      page,
      `?channel=${STA.id}&since=2020-01-01T00:00:00.000Z&until=2020-01-02T00:00:00.000Z`,
    );
    await runReport(page);
    await expect(page.getByText("No messages were received in this period")).toBeVisible();
    await expect(page.getByRole("grid")).toHaveCount(0);
    await expect(totalsTable(page)).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Export CSV…" })).toHaveCount(0);
  });

  test("TR-04 more deliveries than a report lists show a warning asking for a shorter period", async ({
    page,
  }) => {
    await openReport(page, `?channel=${BUSY.id}&range=30d`);
    await runReport(page);
    const warning = main(page).getByRole("alert").filter({ hasText: "The report is incomplete" });
    await expect(warning).toContainText(
      /Only the first 10,000 of [\d,]+ deliveries are listed\. Choose a shorter period to list every one\./,
    );
    // The engine counts the whole period: the totals pass the rows, which stop early.
    await expect(warning).toContainText(
      "Messages received and the totals below cover the whole period",
    );
    expect(await listedCount(page)).toBe(10_000);
    const perDestination = await totals(page);
    expect(perDestination.reduce((sum, each) => sum + (each.counts[0] ?? 0), 0)).toBeGreaterThan(
      10_000,
    );

    // The export says the same, and the file carries it: its last line is the engine's marker.
    await page.getByRole("button", { name: "Export CSV…" }).click();
    const dialog = page.getByRole("dialog", { name: "Export transmission report" });
    const incomplete = dialog.getByRole("alert").filter({ hasText: "This report is incomplete" });
    await expect(incomplete).toContainText(/only the first 10,000 of [\d,]+ are in the report/);
    await expect(incomplete).toContainText("line starting “# truncated:”");
    await expect(incomplete).toContainText("The totals on the page cover the whole period");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    const { text: file } = await exportCsv(page);
    const lines = file.trimEnd().split("\r\n");
    expect(lines).toHaveLength(1 + 10_000 + 2);
    expect(lines.at(-2)).toMatch(
      /^# listed: 10000 of \d+ deliveries of the period were in the report \(limit 10000\); \d+ messages were received$/,
    );
    expect(lines.at(-1)).toBe(
      "# truncated: more than 10000 deliveries in the period; the rows above are the first 10000. Report a shorter period to list every delivery.",
    );
    expect(lines.slice(1, -2).every((line) => !line.startsWith("#"))).toBe(true);

    // A shorter period lists everything, and the warning goes.
    await choose(page, "Period", "Last 24 hours");
    await runReport(page);
    await expect(main(page).getByText("The report is incomplete")).toHaveCount(0);
    expect(await listedCount(page)).toBeLessThan(10_000);
  });
});

test.describe("The report and the audit log", () => {
  test("TR-05 a run is recorded as transmission_verify with the channel and the period", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await runReport(page);
    await mainNav(page).getByRole("link", { name: "Audit" }).click();
    await loaded(page, "Audit");
    await choose(page, "Action", "Transmission verified");
    const outcome = dataRows(page, "Audit events").nth(0);
    const requested = dataRows(page, "Audit events").nth(1);
    await expect(outcome).toContainText("Transmission verified");
    await expect(outcome).toContainText("Succeeded");
    await expect(outcome.getByRole("link", { name: STA.id })).toBeVisible();
    await expect(outcome).toContainText("LAB\\jdoe");
    await expect(requested).toContainText("Requested");
    await outcome.getByRole("gridcell").first().click();
    const panel = eventPanel(page);
    await expect(panel).toContainText("transmission_verify");
    await expect(panel).toContainText(STA.id);
    await expect(panel).toContainText("since");
    await expect(panel).toContainText("messages");
  });
});

test.describe("When the report cannot be run", () => {
  test("TR-06 a channel the engine does not know shows the error with a way to try again", async ({
    page,
  }) => {
    await openReport(page, "?channel=ch-removed");
    await expect(channelBox(page)).toHaveValue(/^ch-removed/);
    await runReport(page);
    const alert = main(page).getByRole("alert").filter({ hasText: "The report could not be run" });
    await expect(alert).toContainText("not found");
    await expect(alert).toContainText("Support reference");
    await expect(page.getByRole("grid")).toHaveCount(0);
    await alert.getByRole("button", { name: "Try again" }).click();
    await expect(alert).toBeVisible();
    // The failure is in the audit log too, as a refusal.
    await mainNav(page).getByRole("link", { name: "Audit" }).click();
    await loaded(page, "Audit");
    await choose(page, "Action", "Transmission verified");
    await expect(dataRows(page, "Audit events").first()).toContainText("Rejected");
  });

  test("TR-06 while the engine is out of reach the report shows the error, and runs once it is back", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await mockHandle(page, (mock) => mock.simulateOutage());
    await expect(
      page.getByRole("alert").filter({ hasText: "Lost the connection to the engine" }),
    ).toBeVisible();
    await runReport(page);
    const alert = main(page).getByRole("alert").filter({ hasText: "The report could not be run" });
    await expect(alert).toBeVisible();
    await expect(alert.getByRole("button", { name: "Try again" })).toBeVisible();
    await mockHandle(page, (mock) => mock.endOutage());
    await alert.getByRole("button", { name: "Try again" }).click();
    await expect(page.getByRole("grid", { name: GRID })).toBeVisible();
    await expect(alert).toHaveCount(0);
  });

  test("TR-06 a custom period needs a start, and ends after it", async ({ page }) => {
    await openReport(page, `?channel=${STA.id}&range=custom`);
    await expect(runButton(page)).toBeDisabled();
    await expect(page.getByText(/Choose when the period starts\./)).toBeVisible();
    await page.goto(
      `/?mock=1&e2e=1&steady=1#/transmission?channel=${STA.id}&since=2026-11-02T00:00:00Z&until=2026-11-01T00:00:00Z`,
    );
    await loaded(page, "Transmission report");
    await expect(runButton(page)).toBeDisabled();
    await expect(page.getByText(/The period must end after it starts\./)).toBeVisible();
  });
});

test.describe("Accessibility", () => {
  test("TR-09 the source endpoint and peer and the configuration and mapping versions can be shown beside each delivery", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await runReport(page);
    await expect(page.getByRole("columnheader", { name: "Source endpoint" })).toHaveCount(0);
    await page.getByText("Show source and versions").click();
    await expect(page.getByRole("checkbox", { name: /Show source and versions/ })).toBeChecked();
    const heads = await page
      .getByRole("grid", { name: GRID })
      .getByRole("columnheader")
      .allTextContents();
    expect(heads.slice(-4)).toEqual([
      "Source endpoint",
      "Source peer",
      "Config version",
      "Mapping version",
    ]);
    const rows = await readRows(page);
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row.at(-4)).toMatch(/^ep-/);
      expect(row.at(-2)).toMatch(/^\d+$/);
      expect(row.at(-1)).toMatch(/^(\d+|No mapping)$/);
    }
    await expectNoAxeViolations(page);
  });

  test("TR-07 the form, a result, the export dialog and the error state have no axe violations", async ({
    page,
  }) => {
    await openReport(page, `?channel=${STA.id}`);
    await expectNoAxeViolations(page);
    await runReport(page);
    await page.getByText("Show sent hash, attempts and first attempt").click();
    await expect(page.getByRole("checkbox", { name: /Show sent hash/ })).toBeChecked();
    await expectNoAxeViolations(page);
    await page.getByRole("button", { name: "Export CSV…" }).click();
    await expect(page.getByRole("dialog", { name: "Export transmission report" })).toBeVisible();
    await expectNoAxeViolations(page);
    await page.getByRole("button", { name: "Cancel" }).click();

    await openReport(page, "?channel=ch-removed");
    await runReport(page);
    await expect(page.getByText("The report could not be run")).toBeVisible();
    await expectNoAxeViolations(page);
  });
});
