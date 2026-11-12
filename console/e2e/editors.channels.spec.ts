import { expect, test } from "@playwright/test";
import {
  expectIssue,
  expectOptional,
  expectRequired,
  fill,
  form,
  general,
  group,
  openEditor,
  pick,
  pickRecord,
  recordValue,
  results,
  reviewDialog,
  runValidate,
  saveAndApply,
  saveAndConfirm,
  select,
  text,
  validate,
} from "./editors.support";

test("H-07 bridge channel: endpoints, pairing strategy and receiver, buffer, retry and tap framer", async ({
  page,
}) => {
  await openEditor(page, "/channels/new", "New channel");
  await expect(page.getByText("Choose a kind to see its settings.")).toBeVisible();
  await pick(page, "Kind", "Bridge");
  await expectRequired(page, [
    "Label",
    "Id",
    "Buffer size (bytes)",
    "Initial backoff (ms)",
    "Maximum backoff (ms)",
  ]);
  await expectOptional(page, ["Maximum attempts"]);
  for (const side of ["Endpoint A", "Endpoint B"]) {
    await expect(recordValue(page, side)).toHaveAttribute("aria-required", "true");
  }
  await expect(text(page, "Buffer size (bytes)")).toHaveValue("65536");
  await fill(page, { Label: "Bench bridge" }, general(page));

  // Both sides are required, and they must differ.
  await validate(page).click();
  await expect(form(page).getByText("Choose endpoint a.")).toBeVisible();
  await expect(form(page).getByText("Choose endpoint b.")).toBeVisible();
  await pickRecord(page, "Endpoint A", "ep-c311");
  await pickRecord(page, "Endpoint B", "ep-c311");
  await validate(page).click();
  await expect(form(page).getByText("Endpoint B must differ from endpoint A.")).toBeVisible();
  await pickRecord(page, "Endpoint B", "ep-mw-1");

  // The pairing strategy: the receiver is asked for only with "Receiver first, then sender".
  await expect(select(page, "Receiver")).toHaveCount(0);
  await pick(page, "Pairing strategy", "Receiver first, then sender");
  await expect(select(page, "Receiver")).toBeVisible();
  await validate(page).click();
  await expect(form(page).getByText("Choose receiver.")).toBeVisible();
  await pick(page, "Receiver", "Endpoint A");
  // A listener strategy needs a TCP listener on one side: the engine says so, with its code.
  await pick(page, "Pairing strategy", "Wait for the listening side, then pair");
  await expect(select(page, "Receiver")).toHaveCount(0);
  await runValidate(page);
  await expectIssue(
    page,
    "This strategy needs one side to be a TCP listener.",
    /channels\[\d+\]\.pairing · listener_strategy/,
  );
  await expect(saveAndApply(page)).toBeDisabled();
  await pickRecord(page, "Endpoint B", "ep-xn1000");
  await runValidate(page);

  // Buffer and retry: client checks, then the engine's retry rule.
  await text(page, "Buffer size (bytes)").fill("0");
  await text(page, "Initial backoff (ms)").fill("5000");
  await text(page, "Maximum backoff (ms)").fill("1000");
  await validate(page).click();
  await expect(form(page).getByText("Enter a whole number, 1 or more.")).toBeVisible();
  await fill(page, { "Buffer size (bytes)": "131072" });
  await runValidate(page);
  await expectIssue(
    page,
    "The longest retry delay must not be shorter than the first.",
    /retry\.maxMs · retry_invalid/,
  );
  await fill(page, { "Maximum backoff (ms)": "60000", "Maximum attempts": "abc" });
  await validate(page).click();
  await expect(form(page).getByText("Enter a whole number, 1 or more.")).toBeVisible();
  await fill(page, { "Maximum attempts": "" });

  await select(page, "Tap framer").click();
  await expect(page.getByRole("option")).toHaveText([
    "None (bytes only)",
    "ASTM",
    "AU",
    "HL7 MLLP",
  ]);
  await page.getByRole("option", { name: "HL7 MLLP" }).click();
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  // The plan: a new channel is started, nothing else restarts.
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Started (new): Bench bridge")).toBeVisible();
  await expect(dialog.getByText(/^Restarted/)).toHaveCount(0);
  await expect(dialog.getByText(/^Untouched: \d+ channels?, which keep running\.$/)).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Journey change");
  await dialog.getByRole("button", { name: /^Apply version/ }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toBeVisible();
  await expect(page).toHaveURL(/#\/channels\/bench-bridge$/);
  await expect(page.getByRole("heading", { level: 1, name: "Bench bridge" })).toBeVisible();
});

test("H-07 bridge tap: retention of the recorded messages, validated, applied and read back", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-br-au5800/edit", "AU5800 chem analyzer to middleware");
  const tap = group(page, "Message tap");
  await expect(select(page, "Tap framer", tap)).toBeVisible();
  // The tap's own default: 30 days, no count limit; the same fields and note as message retention.
  await expect(tap.getByText(/Applies to the messages the tap records/)).toBeVisible();
  await expect(text(page, "Keep for (days)", tap)).toHaveValue("30");
  await expect(text(page, "Keep at most (messages)", tap)).toHaveValue("");

  // The form refuses a day limit of 0; the engine refuses a count below its minimum, with its field.
  await fill(page, { "Keep for (days)": "0" }, tap);
  await validate(page).click();
  await expect(tap.getByText("Enter a whole number, 1 or more.")).toBeVisible();
  await fill(page, { "Keep for (days)": "7", "Keep at most (messages)": "10" }, tap);
  await runValidate(page);
  await expectIssue(
    page,
    "Keep between 1,000 and 1,000,000,000 messages.",
    /tapRetention\.maxMessages/,
  );
  await expect(saveAndApply(page)).toBeDisabled();

  await fill(page, { "Keep for (days)": "7", "Keep at most (messages)": "5000" }, tap);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  // Only the label of a bridge is applied in place: a tap setting restarts it.
  await expect(
    dialog.getByText("Restarted (changed): AU5800 chem analyzer to middleware"),
  ).toBeVisible();
  await dialog
    .getByRole("textbox", { name: /^Change note/ })
    .fill("Journey: keep tap messages 7 days");
  await dialog.getByRole("button", { name: /^Apply version/ }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toBeVisible();

  // The apply returns to the channel page; only then open the editor again (navigating in the
  // page keeps the mock's state) and read the saved values back.
  await expect(page).toHaveURL(/#\/channels\/ch-br-au5800$/);
  await page.evaluate(() => {
    window.location.hash = "/channels/ch-br-au5800/edit";
  });
  await expect(form(page)).toBeVisible();
  const again = group(page, "Message tap");
  await expect(text(page, "Keep for (days)", again)).toHaveValue("7");
  await expect(text(page, "Keep at most (messages)", again)).toHaveValue("5000");
});

test("H-07 message channel: source protocols by endpoint kind, duplicate detection and protocol settings", async ({
  page,
}) => {
  await openEditor(page, "/channels/new", "New channel");
  await pick(page, "Kind", "Message");
  const source = group(page, "Source");
  await expectRequired(page, ["Label", "Id"], general(page));
  await expect(recordValue(page, "Endpoint", source)).toHaveAttribute("aria-required", "true");
  const duplicates = source.getByRole("checkbox", { name: "Detect duplicate messages" });
  await expect(duplicates).toBeChecked();

  // Protocols offered follow the kind of the endpoint.
  const offered = async (label: string) => {
    await select(page, "Protocol", source).click();
    const names = await page.getByRole("option").allTextContents();
    await page.keyboard.press("Escape");
    expect(names, label).toBeDefined();
    return names;
  };
  await pickRecord(page, "Endpoint", "ep-c311", source);
  expect(await offered("serial")).toEqual(["ASTM", "AU"]);
  await pickRecord(page, "Endpoint", "ep-au680", source);
  expect(await offered("listener")).toEqual(["ASTM", "AU", "HL7 v2"]);
  await pickRecord(page, "Endpoint", "ep-qc-in", source);
  expect(await offered("folder")).toEqual(["Delimited files"]);
  await pickRecord(page, "Endpoint", "ep-lis-orders", source);

  // HL7: a source acknowledges; "None" is for destinations only.
  await pick(page, "Protocol", "HL7 v2", source);
  await select(page, "Acknowledgement mode", source).click();
  await expect(page.getByRole("option")).toHaveText(["Original", "Enhanced"]);
  await page.getByRole("option", { name: "Enhanced" }).click();
  // ASTM: the link role.
  await pick(page, "Protocol", "ASTM", source);
  await expect(select(page, "Acknowledgement mode", source)).toHaveCount(0);
  await select(page, "Link role", source).click();
  await expect(page.getByRole("option")).toHaveText(["Computer", "Instrument"]);
  await page.getByRole("option", { name: "Instrument" }).click();
  await expect(source.getByRole("button", { name: "Instrument Link role" })).toBeVisible();

  // Duplicate detection is a plain checkbox.
  await source.locator("label", { hasText: "Detect duplicate messages" }).click();
  await expect(duplicates).not.toBeChecked();
  await source.locator("label", { hasText: "Detect duplicate messages" }).click();
  await expect(duplicates).toBeChecked();

  // A delimited file source starts from a column template, which must be a JSON object.
  await pickRecord(page, "Endpoint", "ep-qc-in", source);
  await pick(page, "Protocol", "Delimited files", source);
  const template = source.getByRole("textbox", { name: /template/i });
  await expect(template).toHaveValue(/\{/);
  await template.fill("[1, 2]");
  await fill(page, { Label: "Bench relay" }, general(page));
  await validate(page).click();
  await expect(
    form(page).getByText("The template must be a JSON object, written with { }."),
  ).toBeVisible();
  await template.fill("{ not json");
  await validate(page).click();
  await expect(form(page).getByText(/^Not valid JSON:/)).toBeVisible();
  void expectIssue;
  void results;
  void saveAndConfirm;
});

test("H-07 message channel: a destination with no mapping and another protocol shows the field error", async ({
  page,
}) => {
  await openEditor(page, "/channels/new", "New channel");
  await pick(page, "Kind", "Message");
  const source = group(page, "Source");
  const destination = group(page, "Destination 1");
  await fill(page, { Label: "Bench passthrough" }, general(page));
  await pickRecord(page, "Endpoint", "ep-lis-orders", source);
  await pick(page, "Protocol", "HL7 v2", source);
  await pickRecord(page, "Endpoint", "ep-lis-out", destination);
  await fill(page, { Label: "To LIS" }, destination);
  await pick(page, "Protocol", "ASTM", destination);
  await pick(page, "Link role", "Computer", destination);

  const rule = /Without a mapping, this destination must use the source's protocol \(HL7 v2\)/;
  await validate(page).click();
  await expect(destination.getByText(rule).first()).toBeVisible();
  // A mapping rule mends it; removing the rule brings the error back.
  await destination.getByRole("button", { name: "Add destination 1 mapping rule" }).click();
  await expect(destination.getByText(rule)).toHaveCount(0);
  await destination.getByRole("button", { name: "Remove destination 1 mapping rule 1" }).click();
  await expect(destination.getByText(rule).first()).toBeVisible();
  // The same protocol as the source passes the original message through.
  await pick(page, "Protocol", "HL7 v2", destination);
  await expect(destination.getByText(rule)).toHaveCount(0);
  await select(page, "Acknowledgement mode", destination).click();
  await expect(page.getByRole("option")).toHaveText(["Original", "Enhanced", "None"]);
  await page.getByRole("option", { name: "None" }).click();
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
});
