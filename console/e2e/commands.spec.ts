import { expect, type Locator, type Page, test } from "@playwright/test";
import { CHANNELS, channelRow, fact, openChannel, openMock, toast } from "./operate.support";
import { heading, loaded, main, mockHandle } from "./support";

type Channel = { id: string; label: string };

const dialog = (page: Page) => page.getByRole("alertdialog");
/** The state chip beside the page title. */
const stateOf = (page: Page) => fact(main(page), "State").first();
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });

/** What the confirmation says before the session closes (src/features/shared-operate/useChannelActions.tsx). */
const PAUSE_BRIDGE =
  /The session closes: both endpoints are released.*No retry is scheduled\. The channel stays paused until you resume it\./s;
const PAUSE_MESSAGE =
  /The sender will see its connection refused\. Messages already accepted stay queued.*No retry is scheduled\./s;
const RESTART_BRIDGE =
  /The session closes and both endpoints are reopened\..*The counters of this instance reset\./s;
const RESTART_MESSAGE =
  /The session closes and the endpoints are reopened\..*The counters of this instance reset\./s;

/** Pauses the channel through the page's own button: asks, then confirms. Returns the toast. */
async function pauseFromPage(page: Page, channel: Channel): Promise<Locator> {
  await button(page, "Pause").click();
  await dialog(page).getByRole("button", { name: "Pause channel" }).click();
  const outcome = toast(page, "Channel paused. No retry is scheduled.");
  await expect(outcome).toBeVisible();
  await expect(outcome).toContainText(channel.label);
  return outcome;
}

async function expectPaused(page: Page): Promise<void> {
  await expect(stateOf(page)).toHaveText("Paused by operator");
  await expect(button(page, "Resume")).toBeVisible();
  await expect(button(page, "Pause")).toHaveCount(0);
  await expect(button(page, "Restart")).toHaveCount(0);
  await expect(main(page).getByText("No retry is scheduled: paused by operator")).toBeVisible();
}

const KINDS = [
  { kind: "bridge", channel: CHANNELS.au5800, pause: PAUSE_BRIDGE, restart: RESTART_BRIDGE },
  { kind: "message", channel: CHANNELS.urisys, pause: PAUSE_MESSAGE, restart: RESTART_MESSAGE },
] as const;

for (const { kind, channel, pause, restart } of KINDS) {
  test(`H-04: pause of a running ${kind} channel asks first and says what will happen`, async ({
    page,
  }) => {
    await openChannel(page, channel);
    await expect(stateOf(page)).toHaveText("Running");

    await button(page, "Pause").click();
    const confirmation = dialog(page);
    await expect(confirmation).toBeVisible();
    await expect(
      confirmation.getByRole("heading", { name: `Pause ${channel.label}?` }),
    ).toBeVisible();
    await expect(confirmation).toContainText(pause);
    // Nothing has happened yet.
    await expect(stateOf(page)).toHaveText("Running");
    await expect(toast(page, /paused/)).toHaveCount(0);

    // Cancel changes nothing.
    await confirmation.getByRole("button", { name: "Cancel" }).click();
    await expect(confirmation).toHaveCount(0);
    await expect(stateOf(page)).toHaveText("Running");
    await expect(button(page, "Pause")).toBeVisible();
    await expect(button(page, "Resume")).toHaveCount(0);
    await expect(toast(page, /paused/)).toHaveCount(0);
  });

  test(`H-04: confirming the pause of a ${kind} channel reports it in words and offers Resume`, async ({
    page,
  }) => {
    await openChannel(page, channel);
    await pauseFromPage(page, channel);
    await expect(dialog(page)).toHaveCount(0);
    await expectPaused(page);
    await expect(heading(page, channel.label)).toBeVisible();
  });

  test(`H-04: resume of a paused ${kind} channel needs no confirmation and says where it starts from`, async ({
    page,
  }) => {
    await openChannel(page, channel);
    await pauseFromPage(page, channel);
    await expectPaused(page);
    const version = /^Version (\d+)$/.exec(
      await fact(main(page), "Active configuration").first().innerText(),
    )?.[1];
    expect(version).toBeDefined();

    await button(page, "Resume").click();
    // Resume runs at once: no question is asked.
    await expect(dialog(page)).toHaveCount(0);
    const outcome = toast(page, `Starting from configuration version ${version}.`);
    await expect(outcome).toBeVisible();
    await expect(outcome).toContainText(channel.label);
    // The channel leaves the paused state and is offered the running channel's actions.
    await expect(stateOf(page)).not.toHaveText("Paused by operator");
    await expect(stateOf(page)).toHaveText(/^(Starting|Running)$/);
    await expect(button(page, "Pause")).toBeVisible();
    await expect(button(page, "Restart")).toBeVisible();
    await expect(button(page, "Resume")).toHaveCount(0);
    await expect(main(page).getByText("No retry is scheduled: paused by operator")).toHaveCount(0);
  });

  test(`H-04: restart of a ${kind} channel asks first, then reports the new instance`, async ({
    page,
  }) => {
    await openChannel(page, channel);
    await button(page, "Restart").click();
    const confirmation = dialog(page);
    await expect(
      confirmation.getByRole("heading", { name: `Restart ${channel.label}?` }),
    ).toBeVisible();
    await expect(confirmation).toContainText(restart);
    await expect(confirmation.getByRole("button", { name: "Restart channel" })).toBeVisible();

    // Cancel changes nothing.
    await confirmation.getByRole("button", { name: "Cancel" }).click();
    await expect(confirmation).toHaveCount(0);
    await expect(toast(page, /Restarting/)).toHaveCount(0);

    const startedBefore = await fact(main(page), "Running since").innerText();
    await button(page, "Restart").click();
    await dialog(page).getByRole("button", { name: "Restart channel" }).click();
    const outcome = toast(page, /^Restarting from configuration version \d+\./);
    await expect(outcome).toBeVisible();
    await expect(outcome).toContainText(channel.label);
    await expect(dialog(page)).toHaveCount(0);
    // A new instance: it starts again, with its own start time.
    await expect(stateOf(page)).toHaveText(/^(Starting|Running)$/);
    await expect(button(page, "Pause")).toBeVisible();
    await expect.poll(() => fact(main(page), "Running since").innerText()).not.toBe(startedBefore);
  });
}

test("H-04: restart resets the counters of the instance", async ({ page }) => {
  const channel = CHANNELS.c503;
  await openChannel(page, channel);
  // A retrying channel has retried; the restarted instance has not.
  await expect(fact(main(page), "Retries")).toHaveText(/^[1-9]\d*$/);
  await button(page, "Restart").click();
  await dialog(page).getByRole("button", { name: "Restart channel" }).click();
  await expect(toast(page, /^Restarting from configuration version \d+\./)).toBeVisible();
  await expect(fact(main(page), "Retries")).toHaveText("0");
});

test.describe("H-04: the engine refuses a command", () => {
  // The page only offers what the engine allows, so a refusal needs a stale view: the second of
  // two requests for the same command. Every request takes 1.5 s (`scenario=slow`), so both are
  // in flight before the first one is carried out and the engine answers the second from the
  // state the first left.
  test("H-04: resuming a channel that is no longer paused shows the engine's reason in words", async ({
    page,
  }) => {
    await openChannel(page, CHANNELS.vitros350, { scenario: "slow" });
    await expect(stateOf(page)).toHaveText("Paused by operator");
    const resume = button(page, "Resume");
    await resume.click();
    await resume.click();

    const refusal = toast(page, `Could not resume ${CHANNELS.vitros350.label}`);
    await expect(refusal).toBeVisible();
    await expect(refusal).toContainText("Not paused");
    // Words, not a generic failure, and the first request did take effect.
    await expect(refusal).not.toContainText("could not complete the action");
    await expect(stateOf(page)).not.toHaveText("Paused by operator");
  });

  test("H-04: a command while the engine is out of reach says so, with the reason", async ({
    page,
  }) => {
    const channel = CHANNELS.urisys;
    await openChannel(page, channel);
    await mockHandle(page, (mock) => mock.simulateOutage());
    await expect(
      page.getByRole("alert").filter({ hasText: "Lost the connection to the engine" }),
    ).toBeVisible();

    await button(page, "Pause").click();
    await dialog(page).getByRole("button", { name: "Pause channel" }).click();
    const refusal = toast(page, `Could not pause ${channel.label}`);
    await expect(refusal).toBeVisible();
    await expect(refusal).toContainText("The connection to the engine was lost.");
    await expect(toast(page, "Channel paused")).toHaveCount(0);
    // The dialog is closed and the channel was not paused.
    await expect(dialog(page)).toHaveCount(0);
    await mockHandle(page, (mock) => mock.endOutage());
    await expect(stateOf(page)).toHaveText("Running");
  });

  test("H-04: a channel stopped for its configuration is not offered a restart", async ({
    page,
  }) => {
    await openChannel(page, CHANNELS.outreach);
    await expect(stateOf(page)).toHaveText("Stopped: configuration invalid");
    await expect(button(page, "Restart")).toHaveCount(0);
    await expect(button(page, "Pause")).toHaveCount(0);
    await expect(button(page, "Resume")).toHaveCount(0);
    await expect(main(page).getByRole("link", { name: "Edit" })).toBeVisible();
  });
});

test.describe("H-04: from the Channels list row menu", () => {
  async function openList(page: Page): Promise<void> {
    await openMock(page, "/channels");
    await loaded(page, "Channels");
    await expect(channelRow(page, "Channels", CHANNELS.urisys.label)).toBeVisible();
  }

  async function openMenu(page: Page, channel: Channel): Promise<Locator> {
    await page.getByRole("button", { name: `Actions for ${channel.label}`, exact: true }).click();
    const menu = page.getByRole("menu");
    await expect(menu).toBeVisible();
    return menu;
  }

  const rowState = (page: Page, channel: Channel) => channelRow(page, "Channels", channel.label);

  test("H-04: the menu offers what the state allows, and Pause asks first", async ({ page }) => {
    const channel = CHANNELS.urisys;
    await openList(page);
    const menu = await openMenu(page, channel);
    await expect(menu.getByRole("menuitem")).toHaveText(["Pause", "Restart", "Edit"]);
    await menu.getByRole("menuitem", { name: "Pause" }).click();

    const confirmation = dialog(page);
    await expect(
      confirmation.getByRole("heading", { name: `Pause ${channel.label}?` }),
    ).toBeVisible();
    await expect(confirmation).toContainText(PAUSE_MESSAGE);
    // Cancel changes nothing.
    await confirmation.getByRole("button", { name: "Cancel" }).click();
    await expect(confirmation).toHaveCount(0);
    await expect(rowState(page, channel)).toContainText("Running");
    await expect(toast(page, /paused/)).toHaveCount(0);

    // Confirm pauses it and reports in words; the row says Paused by operator.
    await (await openMenu(page, channel)).getByRole("menuitem", { name: "Pause" }).click();
    await dialog(page).getByRole("button", { name: "Pause channel" }).click();
    const outcome = toast(page, "Channel paused. No retry is scheduled.");
    await expect(outcome).toBeVisible();
    await expect(outcome).toContainText(channel.label);
    await expect(rowState(page, channel)).toContainText("Paused by operator");
  });

  test("H-04: Resume from the row menu runs at once and the row leaves the paused state", async ({
    page,
  }) => {
    const channel = CHANNELS.vitros350;
    await openList(page);
    await expect(rowState(page, channel)).toContainText("Paused by operator");
    const menu = await openMenu(page, channel);
    await expect(menu.getByRole("menuitem")).toHaveText(["Resume", "Edit"]);
    await menu.getByRole("menuitem", { name: "Resume" }).click();

    await expect(dialog(page)).toHaveCount(0);
    const outcome = toast(page, /^Starting from configuration version \d+\./);
    await expect(outcome).toBeVisible();
    await expect(outcome).toContainText(channel.label);
    await expect(rowState(page, channel)).not.toContainText("Paused by operator");
    await expect(rowState(page, channel)).toContainText(/Starting|Running/);
  });

  test("H-04: Restart from the row menu asks first, then reports the outcome", async ({ page }) => {
    const channel = CHANNELS.au5800;
    await openList(page);
    await (await openMenu(page, channel)).getByRole("menuitem", { name: "Restart" }).click();

    const confirmation = dialog(page);
    await expect(
      confirmation.getByRole("heading", { name: `Restart ${channel.label}?` }),
    ).toBeVisible();
    await expect(confirmation).toContainText(RESTART_BRIDGE);
    await confirmation.getByRole("button", { name: "Cancel" }).click();
    await expect(confirmation).toHaveCount(0);
    await expect(toast(page, /Restarting/)).toHaveCount(0);

    await (await openMenu(page, channel)).getByRole("menuitem", { name: "Restart" }).click();
    await dialog(page).getByRole("button", { name: "Restart channel" }).click();
    const outcome = toast(page, /^Restarting from configuration version \d+\./);
    await expect(outcome).toBeVisible();
    await expect(outcome).toContainText(channel.label);
  });

  test("H-04: a channel stopped for its configuration has no Pause or Restart in the row menu", async ({
    page,
  }) => {
    await openList(page);
    const menu = await openMenu(page, CHANNELS.outreach);
    await expect(menu.getByRole("menuitem")).toHaveText(["Edit"]);
  });

  test("H-04: resuming twice from the row menu shows the engine's reason for the second", async ({
    page,
  }) => {
    const channel = CHANNELS.vitros350;
    await openMock(page, "/channels", { scenario: "slow" });
    await loaded(page, "Channels");
    await expect(rowState(page, channel)).toContainText("Paused by operator");
    await (await openMenu(page, channel)).getByRole("menuitem", { name: "Resume" }).click();
    // The first request is still on its way, so the row still offers Resume.
    await (await openMenu(page, channel)).getByRole("menuitem", { name: "Resume" }).click();

    const refusal = toast(page, `Could not resume ${channel.label}`);
    await expect(refusal).toBeVisible();
    await expect(refusal).toContainText("Not paused");
  });
});
