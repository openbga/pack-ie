import { expect, type Locator, type Page, test } from "@playwright/test";
import { deliveryCard, factValue, notice, openDetail } from "./messages.support";

const original = (page: Page): Locator =>
  page.getByRole("region", { name: "Original message", exact: true });

test("D-12: the original opens only on request, as text, hex and fields", async ({ page }) => {
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const patient = await factValue(page.locator("main"), "Patient ID");
  const viewer = original(page);
  // Not opened yet: no bytes anywhere on the page, and the page says why.
  await expect(
    viewer.getByText(/reads it from the engine's store for this window only/),
  ).toBeVisible();
  await expect(viewer.getByRole("radiogroup")).toHaveCount(0);
  await expect(page.locator("pre")).toHaveCount(0);
  const before = await page.locator("body").innerText();
  expect(before).not.toContain("SYNTH");
  expect(before).not.toContain("<STX>");
  // Text: control characters are named, one record per line.
  await viewer.getByRole("button", { name: "Show original message" }).click();
  const text = viewer.locator("pre");
  await expect(text).toContainText("<STX>");
  await expect(text).toContainText("<CR>");
  await expect(text).toContainText("<ETX>");
  await expect(text).toContainText("<LF>");
  await expect(text).toContainText(`${patient}||SYNTH^PATIENT`);
  expect((await text.innerText()).split("\n").length).toBeGreaterThan(5);
  await expect(viewer.getByRole("radio", { name: "Text" })).toBeChecked();
  await expect(viewer.getByText(/^\d+ B$/)).toBeVisible();
  // Never in the address or the window title.
  for (const place of [page.url(), await page.title()]) {
    expect(place).not.toContain(patient);
    expect(place).not.toContain("<STX>");
    expect(place).not.toContain("SYNTH");
  }
  // Hex: offsets, byte values and the printable characters.
  await viewer.getByRole("radio", { name: "Hex" }).click();
  await expect(viewer.locator("pre")).toContainText("00000000  02 ");
  await expect(viewer.getByRole("button", { name: "Copy as hex" })).toBeVisible();
  // Fields: the engine's list, read only on an explicit, audited Show content.
  await viewer.getByRole("radio", { name: "Fields" }).click();
  await expect(viewer.getByRole("table")).toHaveCount(0);
  await expect(viewer.getByText(/records this in the audit log/)).toBeVisible();
  await viewer.getByRole("button", { name: "Show content" }).click();
  await expect(viewer.getByText("Read by the engine from the original message.")).toBeVisible();
  await expect(viewer.getByRole("columnheader", { name: "Path" })).toBeVisible();
  await expect(viewer.getByRole("columnheader", { name: "Value" })).toBeVisible();
  await expect(viewer.getByRole("rowheader", { name: "P.4" })).toBeVisible();
  await expect(viewer.getByRole("cell", { name: patient, exact: true })).toBeVisible();
  // Hiding forgets the bytes.
  await viewer.getByRole("button", { name: "Hide" }).click();
  await expect(page.locator("pre")).toHaveCount(0);
  await expect(viewer.getByRole("button", { name: "Show original message" })).toBeVisible();
});

/**
 * NOTE: the mock never builds a body over the engine's 64 KB display limit or more than 500
 * fields, so the "cut short" and "more fields exist" wordings cannot be reached from its data.
 * The dev server serves the mock as an ES module the app already loaded (the `?t=` stamp in its
 * address changes as files are edited, so the address is read from the page); this makes its
 * answers say "truncated", which is what the engine would say, without touching `src/`.
 */
/** Picks the Fields view and asks for its content, which the engine records in the audit log. */
async function showFields(viewer: Locator): Promise<void> {
  await viewer.getByRole("radio", { name: "Fields" }).click();
  await viewer.getByRole("button", { name: "Show content" }).click();
}

async function makeAnswersTruncated(page: Page): Promise<void> {
  await page.evaluate(`(async () => {
    const url = performance.getEntriesByType("resource").map((e) => e.name)
      .find((name) => /\\/src\\/api\\/mock\\/index\\.ts/.test(name));
    const proto = (await import(url)).MockEngineApi.prototype;
    const body = proto.getMessageBody;
    proto.getMessageBody = async function (request) {
      const answer = await body.call(this, request);
      return { ...answer, truncated: true, byteLength: 70000 };
    };
    const fields = proto.getMessageFields;
    proto.getMessageFields = async function (id) {
      return { ...(await fields.call(this, id)), truncated: true };
    };
  })()`);
}

// Patches the mock through its dev-server module, so it runs against `npm run dev` only (CI runs
// it in a second pass: .github/workflows/ci.yml).
test("D-12: parsed fields come from the engine, with a find box and its limits said", {
  tag: "@dev-server",
}, async ({ page }) => {
  await openDetail(
    page,
    "?channel=ch-msg-billing",
    /: (Acknowledged|Sending|Queued)/,
    makeAnswersTruncated,
  );
  const viewer = original(page);
  await viewer.getByRole("button", { name: "Show original message" }).click();
  await showFields(viewer);
  // HL7: the paths filters and mappings use.
  await expect(viewer.getByRole("rowheader", { name: "MSH.9.1" })).toBeVisible();
  const count = viewer.getByText(/^\d+ fields$/);
  await expect(count).toBeVisible();
  const all = Number(((await count.textContent()) ?? "").replace(/\D/g, ""));
  expect(all).toBeGreaterThan(10);
  await expect(viewer.getByText("More fields exist than are listed.")).toBeVisible();
  // Find narrows by path or value and says how many of all.
  const find = viewer.getByRole("searchbox", { name: "Find in original message fields" });
  await find.fill("PID.3");
  await expect(viewer.getByText(new RegExp(`^\\d+ of ${all} fields$`))).toBeVisible();
  const paths = await viewer.getByRole("rowheader").allTextContents();
  expect(paths.length).toBeGreaterThan(0);
  expect(paths.every((path) => path.includes("PID.3"))).toBe(true);
  await find.fill("no-such-path-or-value");
  await expect(
    viewer.getByText("No field has a path or value containing 'no-such-path-or-value'."),
  ).toBeVisible();
  // The text view of the same body says it is cut short.
  await viewer.getByRole("radio", { name: "Text" }).click();
  await expect(viewer.getByText("This message is cut short")).toBeVisible();
  await expect(
    viewer.getByText(/more than the engine shows\. Only the beginning is displayed/),
  ).toBeVisible();
});

test("D-12: fields say why none are listed", async ({ page }) => {
  // AU is neither HL7 nor ASTM.
  await openDetail(page, "?channel=ch-msg-au680");
  let viewer = original(page);
  await viewer.getByRole("button", { name: "Show original message" }).click();
  await showFields(viewer);
  await expect(viewer.getByText("Fields are listed for HL7 and ASTM messages.")).toBeVisible();
  await expect(viewer.getByRole("table")).toHaveCount(0);
  // An HL7 message that does not parse.
  await openDetail(page, "?channel=ch-msg-billing&state=invalid&range=any");
  viewer = original(page);
  await viewer.getByRole("button", { name: "Show original message" }).click();
  await showFields(viewer);
  await expect(
    viewer.getByText("The original does not parse as HL7; see the message's reason."),
  ).toBeVisible();
});

test("D-12: each delivery shows its transformed bytes as text and hex, on request", async ({
  page,
}) => {
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const card = deliveryCard(page, "LIS HL7 out");
  await expect(card.getByRole("heading", { level: 4, name: "Transformed message" })).toBeVisible();
  await expect(card.locator("pre")).toHaveCount(0);
  await card.getByRole("button", { name: /^Show transformed message to / }).click();
  // The destination speaks HL7: what it would receive, with the control characters named.
  await expect(card.locator("pre")).toContainText("MSH|^~\\&|");
  await expect(card.locator("pre")).toContainText("<CR>");
  await card.getByRole("radio", { name: "Hex" }).click();
  await expect(card.locator("pre")).toContainText("00000000  4d 53 48 7c");
  await card.getByRole("radio", { name: "Text" }).click();
  await expect(card.locator("pre")).toContainText("MSH|");
  // The original stays closed: each body opens on its own action.
  await expect(original(page).locator("pre")).toHaveCount(0);
  await card.getByRole("button", { name: "Hide" }).click();
  await expect(card.locator("pre")).toHaveCount(0);
});

test("D-12: deliveries list every attempt with time, result, acknowledgement and error code", async ({
  page,
}) => {
  // Several attempts, each ending in a write failure with its error code.
  await openDetail(page, "?delivery=failed", /: Failed/);
  const failed = page.getByRole("article").filter({ hasText: "Failed, not delivered" }).first();
  const table = failed.getByRole("table", { name: /^Attempts to / });
  const heads = await table.getByRole("columnheader").allTextContents();
  expect(heads).toEqual([
    "Attempt",
    "Started",
    "Ended",
    "Result",
    "Acknowledgement code",
    "ACK ID",
    "Error code",
  ]);
  const rows = table.getByRole("row");
  expect(await rows.count()).toBeGreaterThan(1);
  const first = rows.nth(1);
  await expect(first.getByRole("cell").nth(0)).toHaveText("1");
  await expect(first.getByRole("cell").nth(1).locator("time")).toHaveAttribute(
    "datetime",
    /^20\d\d-/,
  );
  await expect(first.getByRole("cell").nth(3)).toHaveText(
    /Write failed|Send timed out|Mapping failed/,
  );
  await expect(first.getByRole("cell").nth(6)).toHaveText(/\w+/);
  // A negative acknowledgement keeps its code.
  await openDetail(page, "?delivery=rejected", /: Rejected/);
  const rejected = page.getByRole("article").filter({ hasText: "Rejected by destination" }).first();
  await expect(rejected.getByRole("row").nth(1)).toContainText("Rejected by the destination");
  await expect(rejected.getByRole("row").nth(1).getByRole("cell").nth(4)).toHaveText(
    /^(AE|AR|CE)$/,
  );
  // The acknowledgement message's own control id is shown beside its code (HL7 only).
  await expect(rejected.getByRole("row").nth(1).getByRole("cell").nth(5)).toHaveText(
    /^(ACK[0-9A-Z]+|None)$/,
  );
  // No answer in time, and a connection lost after the message went out.
  await openDetail(page, "?delivery=timed_out", /: No acknowledgement/);
  const silent = page
    .getByRole("article")
    .filter({ hasText: "No acknowledgement in time" })
    .first();
  await expect(silent.getByRole("row").nth(1)).toContainText("No acknowledgement in time");
  await expect(silent.getByRole("row").nth(1)).toContainText("ack_timeout");
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const lost = page
    .getByRole("article")
    .filter({ hasText: "Outcome unknown, may have been delivered" })
    .first();
  await expect(lost.getByRole("row").nth(1)).toContainText(
    /Connection lost after the message was sent|Engine stopped during the attempt/,
  );
});

test("D-12: the facts of the record: response to the sender, source, configuration version", async ({
  page,
}) => {
  await openDetail(page, "?channel=ch-msg-billing", /: Acknowledged/);
  const facts = page.locator("main");
  expect(await factValue(facts, "Response sent to the sender")).toMatch(/^AA at 20\d\d-/);
  expect(await factValue(facts, "Source endpoint")).not.toBe("");
  expect(await factValue(facts, "Source peer")).toMatch(/^\d+\.\d+\.\d+\.\d+:\d+$/);
  expect(await factValue(facts, "Configuration version")).toMatch(/^\d+$/);
  expect(await factValue(facts, "Channel")).toBe("Billing export");
  expect(await factValue(facts, "Protocol")).toBe("HL7 v2");
  expect(await factValue(facts, "Message type")).toMatch(/^ADT\^A\d\d$/);
  expect(await factValue(facts, "Control ID")).toMatch(/^MC\d+$/);
  expect(await factValue(facts, "Message ID")).toMatch(/^msg-\d+$/);
  // The id in the address is the id on the page.
  expect(page.url()).toContain(`/messages/${await factValue(facts, "Message ID")}`);
});

test("D-12: a message nobody answered says so", async ({ page }) => {
  // Messages read from a file are not answered to a sender.
  await openDetail(page, "?channel=ch-msg-qc");
  expect(await factValue(page.locator("main"), "Response sent to the sender")).toBe("None sent");
  await expect(page.getByText(/Source file/)).toBeVisible();
});

test("D-12: a duplicate says what it repeats and links to the original", async ({ page }) => {
  await openDetail(page, "?state=duplicate");
  await expect(notice(page, "Duplicate message")).toContainText(
    "It repeats an earlier message with the same identity and content. Nothing was sent.",
  );
  await expect(
    page.getByText("This message has no deliveries: nothing was queued for any destination."),
  ).toBeVisible();
  const id = await factValue(page.locator("main"), "Message ID");
  await page.getByRole("link", { name: "The original message" }).click();
  await expect(page).toHaveURL(/#\/messages\/msg-\d+$/);
  expect(page.url()).not.toContain(`/${id}`);
  await expect(page.getByText("Duplicate message")).toHaveCount(0);
});

test("D-12: invalid, aborted and filtered messages say why nothing was sent", async ({ page }) => {
  await openDetail(page, "?channel=ch-msg-billing&state=invalid&range=any");
  let shown = notice(page, "Invalid message");
  await expect(shown).toContainText(
    "The bytes are complete but could not be parsed or validated. Nothing was sent.",
  );
  await expect(shown).toContainText(/\((parse_error|missing_required_field)\)/);
  expect(await factValue(page.locator("main"), "Response sent to the sender")).toMatch(/^AR at /);
  await openDetail(page, "?state=aborted");
  shown = notice(page, "Aborted message");
  await expect(shown).toContainText(
    "The transfer ended before a complete message arrived. Nothing was sent.",
  );
  await expect(shown).toContainText(
    /\((receive_timeout|retry_limit_exceeded|connection_lost|empty_transfer|incomplete_record)\)/,
  );
  await openDetail(page, "?state=filtered");
  shown = notice(page, "Filtered message");
  await expect(shown).toContainText(
    "A filter, or the lack of an enabled destination, excluded it. Nothing was sent.",
  );
  await expect(shown).toContainText(/\((source_filter|destination_filters|no_destination)\)/);
});

test("D-12: unresolved deliveries are called out until resent or closed", async ({ page }) => {
  await openDetail(page, "?delivery=timed_out", /: No acknowledgement/);
  const attention = notice(page, "Needs attention");
  await expect(attention).toContainText(
    /One delivery failed, was rejected, or has no confirmed outcome\./,
  );
  await expect(attention).toContainText("Closing does not contact the destination.");
});
