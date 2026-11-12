import { expect, test } from "@playwright/test";
import {
  addRule,
  expectIssue,
  expectOptional,
  expectRequired,
  fill,
  group,
  mappedChannel,
  optionsOf,
  pick,
  pickRecord,
  row,
  rule,
  runValidate,
  saveAndApply,
  select,
  startMessageChannel,
  text,
  validate,
} from "./editors.support";

const DEST = "Destination 1";
const TABLES = `${DEST} code tables`;

test("H-07 code tables and lookup rules: tables and rows, the lookup's choices and what a missing table does", async ({
  page,
}) => {
  await mappedChannel(page, "Bench tables");
  const tables = group(page, TABLES);
  await expect(
    tables.getByText("No code tables. Add one to use it in a lookup rule."),
  ).toBeVisible();

  // A lookup with no table to choose from says to add one first.
  await addRule(page);
  const lookup = rule(page, "1");
  await pick(page, "Rule", "lookup", lookup);
  await expect(lookup.getByText("Add a code table to this destination below first.")).toBeVisible();

  await tables.getByRole("button", { name: "Add destination 1 code table" }).click();
  const table = row(page, "Destination 1 table 1");
  await expectRequired(page, ["Table name"], table);
  await expect(table.getByText("No rows yet.")).toBeVisible();
  await fill(page, { "Table name": "sex" }, table);
  await table.getByRole("button", { name: "Add row to destination 1 table 1" }).click();
  await table.getByRole("button", { name: "Add row to destination 1 table 1" }).click();
  const row1 = row(page, "Row 1 of destination 1 table 1");
  const row2 = row(page, "Row 2 of destination 1 table 1");
  await expectRequired(page, ["Code"], row1);
  await expectOptional(page, ["Text"], row1);
  await fill(page, { Code: "M", Text: "Male" }, row1);
  await validate(page).click();
  await expect(row2.getByText("Enter code.")).toBeVisible();
  await fill(page, { Code: "F", Text: "Female" }, row2);

  // A second table cannot reuse a name.
  await tables.getByRole("button", { name: "Add destination 1 code table" }).click();
  const second = row(page, "Destination 1 table 2");
  await fill(page, { "Table name": "sex" }, second);
  await validate(page).click();
  await expect(second.getByText("Another table already has this name.").first()).toBeVisible();
  await fill(page, { "Table name": "unit" }, second);

  // The lookup chooses among the tables, and its default is required only when it is used.
  await fill(page, { From: "PID.8", To: "P.8" }, lookup);
  await select(page, "Table", lookup).click();
  await expect(page.getByRole("option")).toHaveText(["sex", "unit"]);
  await page.getByRole("option", { name: "sex", exact: true }).click();
  expect(await optionsOf(page, lookup, "On missing")).toEqual([
    "Fail the message",
    "Keep the code unchanged",
    "Use the default",
    "Write nothing",
  ]);
  await expect(text(page, "Default", lookup)).toHaveCount(0);
  await pick(page, "On missing", "Use the default", lookup);
  await expectRequired(page, ["Default"], lookup);
  await validate(page).click();
  await expect(lookup.getByText("Enter default.")).toBeVisible();
  await fill(page, { Default: "U" }, lookup);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  // Renaming the table the lookup reads leaves the lookup naming a table that is gone.
  await fill(page, { "Table name": "gender" }, table);
  await expect(select(page, "Table", lookup)).toContainText("sex (no such table)");
  await runValidate(page);
  await expectIssue(page, 'Code table "sex" does not exist.', /mappings\[0\]\.table/);
  await expect(saveAndApply(page)).toBeDisabled();
  await pick(page, "Table", "gender", lookup);
  await table.getByRole("button", { name: "Remove row 2 of destination 1 table 1" }).click();
  await expect(row(page, "Row 2 of destination 1 table 1")).toHaveCount(0);
  await tables.getByRole("button", { name: "Remove destination 1 table 2" }).click();
  await expect(row(page, "Destination 1 table 2")).toHaveCount(0);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
});

test("H-07 AU connector: the instrument settings and every rule on them", async ({ page }) => {
  await startMessageChannel(page, "Bench AU");
  const source = group(page, "Source");
  // An AU source over a serial endpoint; nothing is preset (settings come from the parameter sheet).
  await pickRecord(page, "Endpoint", "ep-c311", source);
  await pick(page, "Protocol", "AU", source);
  const au = group(page, "AU instrument settings");
  await expect(
    au.getByText("Copy these from the instrument's online parameter sheet; there are no defaults."),
  ).toBeVisible();
  for (const legend of ["Instrument", "Framing", "Message format", "Timing"]) {
    await expect(au.getByRole("group", { name: legend, exact: true })).toBeVisible();
  }
  await expectRequired(
    page,
    ["Start bytes", "End bytes", "Sample ID digits", "Frame timeout (ms)"],
    au,
  );
  // The block timeout is optional: blank means the engine's default, which the field says.
  await expectOptional(page, ["Block timeout (ms)"], au);
  await expect(au.getByText(/Leave blank for the default, 9900\./)).toBeVisible();
  for (let digit = 1; digit <= 6; digit++) {
    await expectRequired(page, [`Patient information ${digit} digits`], au);
  }
  for (const label of ["Profile", "Maximum wire length", "Class", "Rack digits", "Result digits"]) {
    await expect(select(page, label, au)).toContainText("Select…");
  }
  for (const label of [
    "Check byte (BCC)",
    "ETB between blocks",
    "Unit number",
    "Sex",
    "Age",
    "Month",
  ]) {
    await expect(au.getByRole("checkbox", { name: label, exact: true })).not.toBeChecked();
  }
  expect(await optionsOf(page, au, "Profile")).toEqual(["AU400", "AU640"]);
  expect(await optionsOf(page, au, "Maximum wire length")).toEqual(["256", "512", "1024"]);
  expect(await optionsOf(page, au, "Class")).toEqual(["Class a", "Class b"]);
  expect(await optionsOf(page, au, "Rack digits")).toEqual(["0", "4", "5"]);
  expect(await optionsOf(page, au, "Result digits")).toEqual(["6", "9"]);
  // The test code width is a required choice that starts at the engine's default, 2.
  await expect(select(page, "Test code digits", au)).toContainText("2");
  expect(await optionsOf(page, au, "Test code digits")).toEqual(["2", "3"]);

  // Nothing chosen, nothing typed: every field is asked for.
  await validate(page).click();
  for (const message of [
    "Enter start bytes.",
    "Enter end bytes.",
    "Enter sample id digits.",
    "Enter frame timeout (ms).",
  ]) {
    await expect(au.getByText(message)).toBeVisible();
  }
  // Left blank, the block timeout is accepted.
  await expect(au.getByText("Enter block timeout (ms).")).toHaveCount(0);
  await expect(au.getByText("Enter patient information 1 digits.")).toBeVisible();

  // Control bytes: one or two, 1 to 31, never ACK (6) or NAK (21).
  const bytes = "Enter one or two whole numbers separated by a comma, for example 2 or 13, 10.";
  await fill(page, { "Start bytes": "two", "End bytes": "1, 2, 3" }, au);
  await validate(page).click();
  await expect(au.getByText(bytes)).toHaveCount(2);
  await fill(page, { "Start bytes": "0", "End bytes": "32" }, au);
  await validate(page).click();
  await expect(au.getByText("Each control byte is from 1 to 31.")).toHaveCount(2);
  await fill(page, { "Start bytes": "6", "End bytes": "13, 21" }, au);
  await validate(page).click();
  await expect(au.getByText("Control bytes may not be 6 (ACK) or 21 (NAK).")).toHaveCount(2);
  await fill(page, { "Start bytes": "2", "End bytes": "13, 10" }, au);
  // ETB marks intermediate blocks and only works when the end is the single byte 3.
  await au.locator("label", { hasText: "ETB between blocks" }).click();
  await validate(page).click();
  await expect(au.getByText("ETB can only be used when the end bytes are 3.")).toBeVisible();
  await fill(page, { "End bytes": "3" }, au);
  await validate(page).click();
  await expect(au.getByText("ETB can only be used when the end bytes are 3.")).toHaveCount(0);

  // Digit counts and timeouts.
  await fill(page, { "Sample ID digits": "21", "Patient information 1 digits": "21" }, au);
  await fill(
    page,
    {
      "Patient information 2 digits": "0",
      "Frame timeout (ms)": "150",
      "Block timeout (ms)": "10000",
    },
    au,
  );
  await validate(page).click();
  await expect(au.getByText("Enter a whole number from 1 to 20.")).toBeVisible();
  await expect(au.getByText("Enter a whole number from 0 to 20.")).toHaveCount(1);
  await expect(au.getByText("Enter patient information 3 digits.")).toBeVisible();
  await expect(au.getByText("Enter 100 to 9900 milliseconds, in steps of 100.")).toHaveCount(2);

  // A complete, valid sheet passes the engine's check.
  await pick(page, "Profile", "AU640", au);
  await pick(page, "Maximum wire length", "512", au);
  await pick(page, "Class", "Class b", au);
  await pick(page, "Rack digits", "4", au);
  await pick(page, "Result digits", "6", au);
  await fill(
    page,
    { "Sample ID digits": "13", "Frame timeout (ms)": "1000", "Block timeout (ms)": "" },
    au,
  );
  for (let digit = 1; digit <= 6; digit++) {
    await fill(page, { [`Patient information ${digit} digits`]: "0" }, au);
  }
  // The HL7 destination needs a mapping now that the source speaks AU.
  await addRule(page);
  await fill(page, { From: "R.3", To: "OBX[0].5" }, rule(page, "1"));
  // A blank block timeout is the default and passes the engine's check ...
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  // ... and so does a typed one in range.
  await fill(page, { "Block timeout (ms)": "2000" }, au);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
});
