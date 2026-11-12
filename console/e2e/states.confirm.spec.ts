import { expect, test } from "@playwright/test";
import {
  BRIDGE_CHANNEL,
  confirmDialog,
  createEndpoint,
  ENDPOINT,
  expectEscapeReturnsFocus,
  expectFocusWithin,
  MESSAGE_CHANNEL,
  visit,
} from "./states.support";
import { heading, loaded } from "./support";

interface ChannelActionCase {
  name: string;
  channel: { id: string; label: string };
  action: "Pause" | "Restart";
  says: RegExp;
  confirm: string;
  outcome: string;
}

const CHANNEL_ACTIONS: readonly ChannelActionCase[] = [
  {
    name: "Pause a message channel",
    channel: MESSAGE_CHANNEL,
    action: "Pause",
    says: /The sender will see its connection refused\. Messages already accepted stay queued/,
    confirm: "Pause channel",
    outcome: "Channel paused. No retry is scheduled.",
  },
  {
    name: "Pause a bridge channel",
    channel: BRIDGE_CHANNEL,
    action: "Pause",
    says: /The session closes: both endpoints are released/,
    confirm: "Pause channel",
    outcome: "Channel paused. No retry is scheduled.",
  },
  {
    name: "Restart a message channel",
    channel: MESSAGE_CHANNEL,
    action: "Restart",
    says: /The session closes and the endpoints are reopened\..*The counters of this instance reset/,
    confirm: "Restart channel",
    outcome: "Restarting from configuration version",
  },
  {
    name: "Restart a bridge channel",
    channel: BRIDGE_CHANNEL,
    action: "Restart",
    says: /The session closes and both endpoints are reopened\..*The counters of this instance reset/,
    confirm: "Restart channel",
    outcome: "Restarting from configuration version",
  },
];

test.describe("H-08 confirm: Pause and Restart of a channel say what will happen", () => {
  for (const each of CHANNEL_ACTIONS) {
    test(`H-08 confirm: ${each.name} on the channel page`, async ({ page }) => {
      await visit(page, `/channels/${each.channel.id}`);
      await loaded(page, each.channel.label);
      const trigger = page.getByRole("button", { name: each.action, exact: true });
      await trigger.click();
      const dialog = confirmDialog(page, `${each.action} ${each.channel.label}?`);
      await expect(dialog).toBeVisible();
      await expect(dialog).toContainText(each.says);
      await expect(dialog.getByRole("button", { name: each.confirm })).toBeVisible();
      await expectEscapeReturnsFocus(page, dialog, trigger);

      // Cancel changes nothing: no toast, the same actions are offered.
      await trigger.click();
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await expect(dialog).toHaveCount(0);
      await expect(
        page.getByRole("region", { name: "Notifications" }).getByRole("alert"),
      ).toHaveCount(0);
      await expect(page.getByRole("button", { name: "Resume", exact: true })).toHaveCount(0);
      await expect(trigger).toBeFocused();

      // The primary action does it, and the outcome is told in words.
      await trigger.click();
      await dialog.getByRole("button", { name: each.confirm }).click();
      await expect(dialog).toHaveCount(0);
      await expect(page.getByText(each.outcome)).toBeVisible();
    });
  }

  test("H-08 confirm: Pause and Restart from the row menu of Channels, Escape returns to the row", async ({
    page,
  }) => {
    await visit(page, "/channels");
    const label = MESSAGE_CHANNEL.label;
    for (const action of ["Pause", "Restart"]) {
      await page.getByRole("button", { name: `Actions for ${label}` }).click();
      await page.getByRole("menuitem", { name: action }).click();
      const dialog = confirmDialog(page, `${action} ${label}?`);
      await expect(dialog).toBeVisible();
      await expect(dialog).toContainText(/session closes|sender will see/);
      await expectEscapeReturnsFocus(
        page,
        dialog,
        page.getByRole("gridcell", { name: `Actions for ${label}` }),
      );
      await expect(dialog.getByRole("button", { name: `${action} channel` })).toHaveCount(0);
    }
  });

  test("H-08 confirm: Restart the channels that use an endpoint lists what restarts", async ({
    page,
  }) => {
    await visit(page, `/endpoints/${ENDPOINT.id}`);
    await loaded(page, ENDPOINT.label);
    const trigger = page.getByRole("button", { name: "Restart the channels that use it" });
    await trigger.click();
    const dialog = confirmDialog(page, `Restart the channels that use ${ENDPOINT.label}?`);
    await expect(dialog).toContainText("Each session closes and its endpoints are reopened.");
    await expect(dialog.getByRole("list", { name: "Channels to restart" })).toContainText(
      "Sysmex XN-2000 results",
    );
    await expect(dialog.getByRole("button", { name: /^Restart \d+ channels?$/ })).toBeVisible();
    await expectEscapeReturnsFocus(page, dialog, trigger);
  });
});

test.describe("H-08 confirm: deleting a channel or an endpoint says what is removed", () => {
  test("H-08 confirm: Delete channel explains, Cancel keeps it, Delete removes it", async ({
    page,
  }) => {
    await visit(page, `/channels/${MESSAGE_CHANNEL.id}/edit`);
    await loaded(page, MESSAGE_CHANNEL.label);
    const trigger = page.getByRole("button", { name: "Delete channel…" });
    await trigger.click();
    const dialog = confirmDialog(page, `Delete channel ${MESSAGE_CHANNEL.label}?`);
    await expect(dialog).toContainText(
      /Removes this channel from the configuration as version \d+/,
    );
    await expect(dialog).toContainText("The channel stops; other channels keep running.");
    await expect(dialog).toContainText(/Settings › Versions can restore it/);
    await expectEscapeReturnsFocus(page, dialog, trigger);

    await trigger.click();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`#/channels/${MESSAGE_CHANNEL.id}/edit$`));

    await trigger.click();
    await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Retire the channel");
    await dialog.getByRole("button", { name: "Delete channel" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(heading(page, "Channels")).toBeVisible();
    await expect(
      page.getByRole("button", { name: `Actions for ${MESSAGE_CHANNEL.label}` }),
    ).toHaveCount(0);
  });

  test("H-08 confirm: Delete endpoint refuses an endpoint in use and says which channels use it", async ({
    page,
  }) => {
    await visit(page, `/endpoints/${ENDPOINT.id}/edit`);
    await loaded(page, ENDPOINT.label);
    const trigger = page.getByRole("button", { name: "Delete endpoint…" });
    await trigger.click();
    const dialog = page.getByRole("dialog", { name: `${ENDPOINT.label} is in use` });
    await expect(dialog).toContainText("This endpoint cannot be deleted while channels use it.");
    await expect(dialog.getByRole("link", { name: "Sysmex XN-2000 results" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expectFocusWithin(trigger);
  });

  test("H-08 confirm: Delete endpoint explains, Cancel keeps it, Delete removes it", async ({
    page,
  }) => {
    await createEndpoint(page, "Scratch LIS", "scratch-lis");
    await visit(page, "/endpoints/scratch-lis/edit");
    await loaded(page, "Scratch LIS");
    const trigger = page.getByRole("button", { name: "Delete endpoint…" });
    await trigger.click();
    const dialog = confirmDialog(page, "Delete endpoint Scratch LIS?");
    await expect(dialog).toContainText(
      /Removes this endpoint from the configuration as version \d+/,
    );
    await expect(dialog).toContainText("No channel uses it, so no channel is restarted.");
    await expectEscapeReturnsFocus(page, dialog, trigger);

    await trigger.click();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page).toHaveURL(/#\/endpoints\/scratch-lis\/edit$/);

    await trigger.click();
    await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Retire the endpoint");
    await dialog.getByRole("button", { name: "Delete endpoint" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(heading(page, "Endpoints")).toBeVisible();
    await expect(page.getByRole("button", { name: "Actions for Scratch LIS" })).toHaveCount(0);
  });
});
