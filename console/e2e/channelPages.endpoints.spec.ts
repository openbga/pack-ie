import { expect, type Page, test } from "@playwright/test";
import { COUNT, region } from "./channelPages.support";
import { CHANNELS, fact, lastErrorNote, openChannel, openMock } from "./operate.support";
import { dataRows, loaded, main } from "./support";

test.describe("H-03: endpoint pages", () => {
  /** Opens the Endpoints list and then the endpoint called `label`, by clicking its row. */
  async function openFromList(page: Page, label: string): Promise<void> {
    await openMock(page, "/endpoints");
    await loaded(page, "Endpoints");
    const row = dataRows(page, "Endpoints").filter({
      has: page.getByRole("gridcell", { name: label, exact: true }),
    });
    await row.click();
    await loaded(page, label);
  }

  async function expectCommonFacts(page: Page, kind: string, points: RegExp) {
    const facts = main(page);
    await expect(fact(facts, "Endpoint id")).toHaveText(/^ep-/);
    await expect(fact(facts, "Kind")).toHaveText(kind);
    await expect(fact(facts, "Points to")).toHaveText(points);
    await expect(fact(facts, "In this state for")).toHaveText(/\(since .+\)/);
    await expect(fact(facts, "Held by")).not.toBeEmpty();
    await expect(fact(facts, "Active peer")).not.toBeEmpty();
    // The last error's own context repeats the streak and comes first on the page.
    await expect(fact(facts, "Failure streak").last()).toHaveText(COUNT);
    await expect(fact(facts, "Next retry").last()).not.toBeEmpty();
    await expect(fact(facts, "Last activity")).toContainText(/just now|ago/);
    const counters = page.getByRole("table", { name: "Endpoint counters" });
    await expect(counters.getByRole("row", { name: /^In\b/ })).toBeVisible();
    await expect(counters.getByRole("row", { name: /^Out\b/ })).toBeVisible();
    await expect(page.getByRole("table", { name: "Channels using this endpoint" })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Restart the channels that use it" }),
    ).toBeVisible();
  }

  test("H-03: an endpoint in error shows its state, streak, next retry, error and next action", async ({
    page,
  }) => {
    await openFromList(page, "ACL TOP 550 coag (COM9)");
    await expectCommonFacts(page, "Serial", /^COM9/);
    await expect(fact(main(page), "State")).toHaveText("Error");
    await expect(fact(main(page), "Failure streak").last()).toHaveText(/^[1-9]\d*$/);
    await expect(fact(main(page), "In this state for")).toContainText(/^\d+ h/);

    const note = lastErrorNote(page);
    await expect(note).toContainText("COM9: port not found");
    await expect(fact(note, "Failure streak")).toHaveText(/^[1-9]\d*$/);
    await expect(fact(note, "Time")).toContainText(/\(.+ ago\)/);
    await expect(fact(note, "Retry")).toContainText("Next attempt");
    await expect(note.getByText("Next step:")).toBeVisible();
    await expect(note).toContainText("Check the USB-serial adapter");

    const alerts = page.getByRole("list", { name: "Active alerts of this endpoint" });
    await expect(alerts).toContainText("Critical");
    // The channel that uses it, and that it does not hold the endpoint while it is failing.
    const using = page.getByRole("table", { name: "Channels using this endpoint" });
    const row = using.getByRole("row").filter({ hasText: "ACL TOP 550 coag to middleware" });
    await expect(row).toContainText("Bridge side A");
    await expect(row).toContainText("Retrying");
  });

  test("H-03: a listener shows who holds it and the connected peer", async ({ page }) => {
    await openFromList(page, "AU680 chem analyzer");
    await expectCommonFacts(page, "TCP listener", /^listen 10\.0\.4\.5:5001$/);
    await expect(fact(main(page), "State")).toHaveText("Connected");
    await expect(
      fact(main(page), "Held by").getByRole("link", { name: "AU680 results" }),
    ).toBeVisible();
    await expect(fact(main(page), "Active peer")).toHaveText(/^10\.0\.4\.41:\d+$/);
    await expect(fact(main(page), "Failure streak")).toHaveText("0");
    await expect(fact(main(page), "Next retry")).toHaveText("None scheduled");
    // Healthy: no error block.
    await expect(page.getByText("Last error", { exact: true })).toHaveCount(0);
    const using = page.getByRole("table", { name: "Channels using this endpoint" });
    const row = using.getByRole("row").filter({ hasText: "AU680 results" });
    await expect(row).toContainText("Source");
    await expect(row).toContainText("Yes");
  });

  test("H-03: a listener that cannot listen shows its error and what to do", async ({ page }) => {
    await openFromList(page, "Sysmex XN-2000");
    await expectCommonFacts(page, "TCP listener", /^listen 10\.0\.4\.5:5003$/);
    await expect(fact(main(page), "State")).toHaveText("Error");
    const note = lastErrorNote(page);
    await expect(note).toContainText("cannot accept connections: 10.0.4.5:5003 is already in use");
    await expect(note).toContainText("Find the program that uses port 5003");
    await expect(fact(note, "Failure streak")).toHaveText(/^[1-9]\d*$/);
  });

  test("H-03: a folder endpoint shows its path, counters and the channels that write to it", async ({
    page,
  }) => {
    await openFromList(page, "Results archive folder");
    await expectCommonFacts(page, "Folder", /^D:\\LabData\\Results\\Archive$/);
    await expect(fact(main(page), "State")).toHaveText("Connected");
    await expect(fact(main(page), "Held by")).toHaveText("No channel");
    await expect(fact(main(page), "Active peer")).toHaveText("None");
    const using = page.getByRole("table", { name: "Channels using this endpoint" });
    await expect(using.getByRole("row").filter({ hasText: "AU680 results" })).toContainText(
      "Destination",
    );
    await expect(using.getByRole("row").filter({ hasText: "QC results archive" })).toBeVisible();
  });

  test("H-03: an SFTP endpoint shows its address without secrets and its host as the peer", async ({
    page,
  }) => {
    await openFromList(page, "Orders folder (SFTP)");
    await expectCommonFacts(
      page,
      "SFTP",
      /^sftp:\/\/labxfer@sftp\.partner\.example:22\/outbound\/orders$/,
    );
    await expect(fact(main(page), "State")).toHaveText("Connected");
    await expect(fact(main(page), "Active peer")).toHaveText("sftp.partner.example:22");
    // The settings summary never shows the secret's name or value.
    await expect(main(page).getByText(/orders-sftp-key|password/i)).toHaveCount(0);
    await expect(fact(main(page), "Held by")).toHaveText("No channel");
    const using = page.getByRole("table", { name: "Channels using this endpoint" });
    await expect(using.getByRole("row").filter({ hasText: "Orders folder import" })).toBeVisible();
  });

  test("H-03: an SFTP endpoint in error names the failed login and the next action", async ({
    page,
  }) => {
    await openFromList(page, "Reference lab SFTP");
    await expect(fact(main(page), "State")).toHaveText("Error");
    const note = lastErrorNote(page);
    await expect(note).toContainText("SFTP authentication failed for user labxfer");
    await expect(note).toContainText("Re-enter the password under Settings, Secrets");
    await expect(note.getByText("Next step:")).toBeVisible();
  });
});

test.describe("H-03: live transcript", () => {
  const segments = (page: Page, pane: string) =>
    page.getByRole("log", { name: `Transcript ${pane}` }).getByRole("listitem");

  /** Waits for at least one engine tick: a running bridge's byte counter moves on. */
  async function afterTick(page: Page): Promise<void> {
    const bytes = () =>
      page
        .getByRole("table", { name: "Bytes by direction" })
        .getByRole("row")
        .nth(1)
        .getByRole("cell")
        .nth(1)
        .innerText();
    const before = await bytes();
    await expect.poll(bytes).not.toBe(before);
  }

  test("H-03: a bridge transcript starts, shows two timestamped panes, freezes and stops", async ({
    page,
  }) => {
    const channel = CHANNELS.au5800;
    await openChannel(page, channel);
    const panel = region(page, "Live transcript");
    await expect(segments(page, "A → B")).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Freeze" })).toBeDisabled();

    await panel.getByRole("button", { name: "Start live transcript" }).click();
    await expect(panel.getByRole("button", { name: "Stop live transcript" })).toBeVisible();
    await expect(panel.getByText(/^Showing live traffic\./)).toBeVisible();

    // Two panes, each with segments that carry the time they were captured at.
    for (const pane of ["A → B", "B → A"]) {
      await expect(panel.getByRole("heading", { level: 3, name: pane })).toBeVisible();
      await expect(segments(page, pane).first()).toBeVisible({ timeout: 20_000 });
      const stamp = segments(page, pane).first().locator("time");
      await expect(stamp).toHaveText(/^\d{1,2}:\d{2}:\d{2}/);
      await expect(stamp).toHaveAttribute("datetime", /^\d{4}-\d{2}-\d{2}T/);
    }
    await expect(panel.getByText(/^\d+ segments?$/).first()).toBeVisible();

    // Freeze stops the display; capture goes on and is counted.
    await panel.getByRole("button", { name: "Freeze" }).click();
    await expect(panel.getByRole("button", { name: "Unfreeze" })).toBeVisible();
    const shown = await segments(page, "A → B").count();
    await expect(
      panel.getByText(/^Display frozen; [1-9]\d* new segments? captured\.$/),
    ).toBeVisible();
    await afterTick(page);
    expect(await segments(page, "A → B").count()).toBe(shown);
    // Unfreezing shows what was captured meanwhile.
    await panel.getByRole("button", { name: "Unfreeze" }).click();
    await expect(panel.getByText(/^Display frozen/)).toHaveCount(0);
    await expect.poll(() => segments(page, "A → B").count()).toBeGreaterThan(shown);

    // Stop ends the capture: the engine is told, nothing more arrives, the panel says it is off.
    await panel.getByRole("button", { name: "Stop live transcript" }).click();
    await expect(panel.getByRole("button", { name: "Start live transcript" })).toBeVisible();
    await expect(panel.getByText(/^The transcript is off\./)).toBeVisible();
    const kept = await segments(page, "A → B").count();
    await afterTick(page);
    await afterTick(page);
    expect(await segments(page, "A → B").count()).toBe(kept);
    // The one viewer was released: it can be started again.
    await panel.getByRole("button", { name: "Start live transcript" }).click();
    await expect(panel.getByRole("button", { name: "Stop live transcript" })).toBeVisible();
    await expect(page.getByText("The transcript could not be started")).toHaveCount(0);
    await expect.poll(() => segments(page, "A → B").count()).toBeGreaterThan(kept);
  });

  test("H-03: a message channel transcript has a Source and a Destinations pane", async ({
    page,
  }) => {
    await openChannel(page, CHANNELS.urisys);
    const panel = region(page, "Live transcript");
    await panel.getByRole("button", { name: "Start live transcript" }).click();
    for (const pane of ["Source", "Destinations"]) {
      await expect(panel.getByRole("heading", { level: 3, name: pane })).toBeVisible();
      await expect(segments(page, pane).first()).toBeVisible({ timeout: 20_000 });
      await expect(segments(page, pane).first().locator("time")).toHaveText(/^\d{1,2}:\d{2}:\d{2}/);
    }
    await panel.getByRole("button", { name: "Stop live transcript" }).click();
    await expect(panel.getByRole("button", { name: "Start live transcript" })).toBeVisible();
  });
});
