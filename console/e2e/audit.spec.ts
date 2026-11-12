import { expect, test } from "@playwright/test";
import {
  COLUMN,
  column,
  countText,
  loadedCount,
  openAudit,
  readRows,
  rowOfAction,
  verification,
  verifyChain,
  visibleColumn,
} from "./audit.support";
import { downloadFrom, downloadText } from "./download.support";
import { CHANNELS, toast } from "./operate.support";
import {
  choose,
  dataRows,
  eventPanel,
  heading,
  loaded,
  mockHandle,
  navigateTo,
  open,
  param,
} from "./support";

test.use({ timezoneId: "UTC" });

const CSV_HEADER =
  "seq,at,actor_uid,actor_name,origin,action,object_type,object_id,outcome,details,prev_hash,hash";

test.describe("Audit list", () => {
  test("lists the newest events first, a page at a time", async ({ page }) => {
    await openAudit(page);
    await expect(countText(page)).toHaveText("100 events loaded · older events available");
    const times = await visibleColumn(page, "time");
    expect(times.length).toBeGreaterThan(5);
    expect(times).toEqual([...times].sort().reverse());
    for (const time of times) expect(time).toMatch(/^\d\d-\d\d \d\d:\d\d:\d\d$/);

    await page.getByRole("button", { name: "Load older events" }).click();
    await expect.poll(() => loadedCount(page)).toBe(200);
    await expect(countText(page)).toContainText("older events available");
  });

  test("a filter by action and actor narrows the list and survives a reload", async ({ page }) => {
    await openAudit(page);
    const all = await loadedCount(page);
    await choose(page, "Action", "Channel paused");
    await expect.poll(() => param(page, "action")).toBe("channel_pause");
    await page.getByRole("searchbox", { name: "Actor" }).fill("LAB\\jdoe");
    await page.getByRole("searchbox", { name: "Actor" }).press("Enter");
    await expect.poll(() => param(page, "actor")).toBe("LAB\\jdoe");
    // The list shows the previous rows until the engine answers: wait for the narrowed set.
    await expect
      .poll(async () => [...new Set(column(await readRows(page), "actor"))])
      .toEqual(["LAB\\jdoe"]);
    await expect(countText(page)).toContainText("no older events");
    const narrowed = await loadedCount(page);
    expect(narrowed).toBeGreaterThan(0);
    expect(narrowed).toBeLessThan(all);
    const rows = await readRows(page);
    expect(rows).toHaveLength(narrowed);
    expect(new Set(column(rows, "action"))).toEqual(new Set(["Channel paused"]));

    await page.reload();
    await loaded(page, "Audit");
    await expect(page.getByRole("button", { name: /Channel paused Action$/ })).toBeVisible();
    await expect(page.getByRole("searchbox", { name: "Actor" })).toHaveValue("LAB\\jdoe");
    await expect(countText(page)).toContainText(`${narrowed} events loaded`);

    await page.getByRole("button", { name: "Clear filters" }).click();
    await expect.poll(() => param(page, "action")).toBeNull();
    await expect(countText(page)).toContainText("100 events loaded");
  });

  test("typing an actor right after choosing an action keeps both filters, on a slow computer", async ({
    page,
  }) => {
    // The address changes at once and the page follows; a slow page used to lose the action.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 6 });
    await openAudit(page);
    await choose(page, "Action", "Channel paused");
    await page.getByRole("searchbox", { name: "Actor" }).fill("LAB\\jdoe");
    await page.getByRole("searchbox", { name: "Actor" }).press("Enter");
    await expect.poll(() => param(page, "actor")).toBe("LAB\\jdoe");
    expect(param(page, "action")).toBe("channel_pause");
    await expect(page.getByRole("button", { name: /Channel paused Action$/ })).toBeVisible();
  });

  test("a filter that excludes everything says so, and says actor and object ID match exactly", async ({
    page,
  }) => {
    await open(page, "/audit?actor=jdoe");
    await loaded(page, "Audit");
    await expect(page.getByText("No audit events match the filters")).toBeVisible();
    await expect(page.getByText(/Actor and object ID must match exactly/)).toBeVisible();
    await page.getByRole("button", { name: "Clear filters" }).first().click();
    await expect(dataRows(page, "Audit events").first()).toBeVisible();
  });

  test("an action done in the console is listed newest, with its request before it", async ({
    page,
  }) => {
    await open(page, `/channels/${CHANNELS.urisys.id}`);
    await loaded(page, CHANNELS.urisys.label);
    await page.getByRole("main").getByRole("button", { name: "Pause", exact: true }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Pause channel" }).click();
    await expect(toast(page, "Channel paused. No retry is scheduled.")).toBeVisible();

    await navigateTo(page, "Audit");
    await loaded(page, "Audit");
    const first = dataRows(page, "Audit events").nth(0);
    const second = dataRows(page, "Audit events").nth(1);
    await expect(first.getByRole("gridcell").nth(COLUMN.action)).toHaveText("Channel paused");
    await expect(first.getByRole("gridcell").nth(COLUMN.outcome)).toContainText("Succeeded");
    await expect(first.getByRole("gridcell").nth(COLUMN.actor)).toHaveText("LAB\\jdoe");
    await expect(first.getByRole("link", { name: CHANNELS.urisys.id })).toBeVisible();
    await expect(second.getByRole("gridcell").nth(COLUMN.action)).toHaveText("Channel paused");
    await expect(second.getByRole("gridcell").nth(COLUMN.outcome)).toContainText("Requested");
  });
});

test.describe("Audit detail", () => {
  test("a row opens the event with its notes in full, keeps it in the address and closes with Escape", async ({
    page,
  }) => {
    await openAudit(page, "?action=config_apply");
    await rowOfAction(page, "Configuration applied").click();
    const panel = eventPanel(page);
    await expect(panel).toBeVisible();
    const seq = param(page, "seq");
    expect(seq).toMatch(/^\d+$/);
    await expect(panel).toHaveAccessibleName(`Event ${seq}`);
    await expect(panel.getByText("config_apply", { exact: true })).toBeVisible();
    await expect(panel.getByText("Note", { exact: true })).toBeVisible();
    await expect(panel.getByText("base_version", { exact: true })).toBeVisible();
    await expect(panel).toContainText(
      /Raise Analyzer 2 baud rate after cable swap|Add LIS destination for hematology|Change request CR-1042: retention to 90 days/,
    );
    await expect(panel.getByText(/^[0-9a-f]{64}$/)).toHaveCount(2);

    await page.reload();
    await loaded(page, "Audit");
    await expect(eventPanel(page)).toHaveAccessibleName(`Event ${seq}`);

    await page.getByRole("button", { name: "Next event (older)" }).click();
    await expect(eventPanel(page)).not.toHaveAccessibleName(`Event ${seq}`);
    await page.keyboard.press("Escape");
    await expect(eventPanel(page)).toHaveCount(0);
    await expect.poll(() => param(page, "seq")).toBeNull();
  });
});

test.describe("Audit export", () => {
  test("exports the filtered set as CSV, announces it, and records the export", async ({
    page,
  }) => {
    await openAudit(page, "?action=config_apply");
    await expect(countText(page)).toContainText("no older events");
    const shown = await loadedCount(page);
    expect(shown).toBeGreaterThan(0);

    await page.getByRole("button", { name: "Export…" }).click();
    const dialog = page.getByRole("dialog", { name: "Export audit events" });
    await expect(dialog).toContainText("The export is recorded in the audit log");
    await expect(dialog).toContainText("Filters in effect: action Configuration applied.");
    const file = await downloadFrom(page, dialog.getByRole("button", { name: "Export" }));
    expect(file.suggestedFilename()).toBe("packie-audit.csv");
    const lines = (await downloadText(file)).trimEnd().split("\n");
    expect(lines[0]).toBe(CSV_HEADER);
    expect(lines.length - 1).toBe(shown);
    for (const line of lines.slice(1)) expect(line.split(",")[5]).toBe("config_apply");

    await expect(dialog).toHaveCount(0);
    await expect(toast(page, "Audit log exported")).toContainText(
      `${shown} events saved to packie-audit.csv.`,
    );

    await page.getByRole("button", { name: "Clear filters" }).click();
    const newest = dataRows(page, "Audit events").first();
    await expect(newest.getByRole("gridcell").nth(COLUMN.action)).toHaveText("Audit log exported");
  });

  test("refuses to export while the audit log cannot be written, and works again once it can", async ({
    page,
  }) => {
    await openAudit(page);
    await mockHandle(page, (handle) => handle.setAuditAvailable(false));
    await page.getByRole("button", { name: "Export…" }).click();
    const dialog = page.getByRole("dialog", { name: "Export audit events" });
    await dialog.getByRole("button", { name: "Export" }).click();
    // The patient-information warning is an alert too: this one is the failure.
    const alert = dialog.getByRole("alert").filter({ hasText: "The export failed" });
    await expect(alert).toContainText("The export failed");
    await expect(alert).toContainText("The audit log cannot be written.");
    await mockHandle(page, (handle) => handle.setAuditAvailable(true));
    await downloadFrom(page, dialog.getByRole("button", { name: "Export" }));
    await expect(dialog).toHaveCount(0);
  });
});

test.describe("Audit chain verification", () => {
  test("verifies the chain, then reports a pruned start, then a broken chain", async ({ page }) => {
    await openAudit(page);
    const ok = await verifyChain(page, "Chain verified: OK");
    await expect(ok).toContainText(/Head hash\s*[0-9a-f]{64}/);
    await expect(ok).toContainText("audit.anchor");
    await expect(ok).not.toContainText("retention removed older files");
    // Verifying is recorded: the list now starts with it.
    await expect(
      dataRows(page, "Audit events").first().getByRole("gridcell").nth(COLUMN.action),
    ).toHaveText("Audit chain verified");

    await mockHandle(page, (handle) => handle.pruneAudit(50));
    const pruned = await verifyChain(page, "The chain starts at event 51");
    await expect(pruned).toContainText(
      "The chain starts at event 51 because retention removed older files (recorded as audit_prune); the rest is intact.",
    );
    await expect(pruned).toContainText("Chain verified: OK");

    await mockHandle(page, (handle) => handle.breakAuditChain(7));
    const broken = await verifyChain(page, "Chain broken at seq 7");
    await expect(broken).toContainText("An event was edited");
    await expect(broken).toContainText("seq 7");
    await expect(broken).toContainText("audit.sqlite3");
    await expect(broken).not.toContainText("Chain verified: OK");
    await expect(verification(page)).toHaveCount(1);
  });

  test("refuses to verify while the audit log cannot be written", async ({ page }) => {
    await openAudit(page);
    await mockHandle(page, (handle) => handle.setAuditAvailable(false));
    await page.getByRole("button", { name: "Verify chain" }).click();
    await expect(page.getByText("The chain could not be verified")).toBeVisible();
    await mockHandle(page, (handle) => handle.setAuditAvailable(true));
    await page.getByRole("button", { name: "Try again" }).click();
    await expect(verification(page)).toContainText("Chain verified: OK");
  });
});

test("the Audit page has a heading and a navigation entry", async ({ page }) => {
  await open(page, "/dashboard");
  await navigateTo(page, "Audit");
  await expect(heading(page, "Audit")).toBeVisible();
});
