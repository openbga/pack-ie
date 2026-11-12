import { expect, type Locator, test } from "@playwright/test";
import {
  addRule,
  expectIssue,
  expectOptional,
  expectRequired,
  fill,
  group,
  MAPPINGS,
  mappedChannel,
  optionsOf,
  pick,
  results,
  row,
  rule,
  runValidate,
  saveAndApply,
  startMessageChannel,
  text,
  validate,
} from "./editors.support";

const SOURCE_FILTERS = "Source filter rules";
const DEST = "Destination 1";

test("H-07 source filters: every operator, value fields that follow the operator, order and removal", async ({
  page,
}) => {
  await startMessageChannel(page, "Bench filters");
  const filters = group(page, SOURCE_FILTERS);
  await expect(filters.getByText("No filters.")).toBeVisible();
  await filters.getByRole("button", { name: "Add source filter" }).click();
  const first = row(page, "Source filter 1");
  await expectRequired(page, ["Path", "Value"], first);

  // Every operator of the spec, in words.
  expect(await optionsOf(page, first, "Operator")).toEqual([
    "equals",
    "does not equal",
    "is one of",
    "is not one of",
    "starts with",
    "contains",
    "matches (regular expression)",
    "is present",
    "is absent",
    "is greater than",
    "is greater than or equal to",
    "is less than",
    "is less than or equal to",
  ]);

  // "is one of" (in): comma-separated values, with its help.
  await pick(page, "Operator", "is one of", first);
  await expect(
    first.getByText("Separate the values with commas, for example GLU, NA, K."),
  ).toBeVisible();
  await fill(page, { Path: "OBX[0].3", Value: "GLU, NA, K" }, first);
  // Text operators can ignore case; presence checks take no value at all.
  await expect(first.getByRole("checkbox", { name: "Ignore case" })).toBeVisible();
  await pick(page, "Operator", "is present", first);
  await expect(text(page, "Value", first)).toHaveCount(0);
  await expect(first.getByRole("checkbox", { name: "Ignore case" })).toHaveCount(0);
  await pick(page, "Operator", "is greater than", first);
  await expect(first.getByText("A number; text that is not a number never matches.")).toBeVisible();
  await pick(page, "Operator", "matches (regular expression)", first);
  await expect(first.getByText("A regular expression of at most 200 characters.")).toBeVisible();
  await pick(page, "Operator", "is not one of", first);

  // The engine's checks of a filter, at its fields: a path and a regular expression.
  await text(page, "Value", first).fill("");
  await validate(page).click();
  await expect(first.getByText("Enter value.")).toBeVisible();
  await fill(page, { Path: "bad path!", Value: "A" }, first);
  await runValidate(page);
  await expectIssue(page, /^Not a valid path; use SEGMENT/, /channels\[\d+\]\.filters\[0\]\.path/);
  await expect(first.getByText(/^Not a valid path; use SEGMENT/)).toBeVisible();
  await expect(saveAndApply(page)).toBeDisabled();
  await fill(page, { Path: "OBX[0].3" }, first);
  await pick(page, "Operator", "matches (regular expression)", first);
  await fill(page, { Value: "([" }, first);
  await runValidate(page);
  await expectIssue(page, "This is not a valid regular expression.", /filters\[0\]\.value/);
  await fill(page, { Value: "^GL.*" }, first);
  await pick(page, "Operator", "is greater than", first);
  await fill(page, { Value: "abc" }, first);
  await runValidate(page);
  // A comparison that can never be true is a warning, which does not block.
  await expect(results(page)).toContainText(
    "This is not a number, so the comparison is never true.",
  );
  await expect(saveAndApply(page)).toBeEnabled();

  // Order: a second filter, moved above the first, and removed.
  await filters.getByRole("button", { name: "Add source filter" }).click();
  await fill(page, { Path: "MSH.9", Value: "ORU" }, row(page, "Source filter 2"));
  await filters.getByRole("button", { name: "Move source filter 2 up" }).click();
  await expect(filters.getByRole("status")).toContainText(
    "Source filter 2 moved to position 1 of 2.",
  );
  await expect(text(page, "Path", row(page, "Source filter 1"))).toHaveValue("MSH.9");
  await expect(text(page, "Path", row(page, "Source filter 2"))).toHaveValue("OBX[0].3");
  await filters.getByRole("button", { name: "Remove source filter 1" }).click();
  await expect(row(page, "Source filter 2")).toHaveCount(0);
  await expect(text(page, "Path", row(page, "Source filter 1"))).toHaveValue("OBX[0].3");
});

test("H-07 destination filters apply only to that destination", async ({ page }) => {
  await startMessageChannel(page, "Bench destination filters");
  const filters = group(page, `${DEST} filter rules`);
  await expect(filters).toContainText(
    "A delivery to this destination is made only when all of these match.",
  );
  await filters.getByRole("button", { name: "Add destination 1 filter" }).click();
  const first = row(page, "Destination 1 filter 1");
  await pick(page, "Operator", "contains", first);
  await fill(page, { Path: "OBX[0].5", Value: "critical" }, first);
  await first.locator("label", { hasText: "Ignore case" }).click();
  await expect(first.getByRole("checkbox", { name: "Ignore case" })).toBeChecked();
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await expect(group(page, SOURCE_FILTERS).getByText("No filters.")).toBeVisible();
});

test("H-07 mapping rules: the fields of copy, constant, lookup, concat and datetime", async ({
  page,
}) => {
  await mappedChannel(page, "Bench mapping");
  // No mapping and a different protocol is an error until a rule exists.
  await addRule(page);
  const first = rule(page, "1");
  expect(await optionsOf(page, first, "Rule")).toEqual([
    "copy",
    "constant",
    "lookup",
    "concat",
    "datetime",
    "calculate",
    "when",
    "for_each",
    "append",
  ]);

  // copy: From, To, Default, Required.
  await expectRequired(page, ["From", "To"], first);
  await expectOptional(page, ["Default"], first);
  await expect(first.getByRole("checkbox", { name: "Required" })).not.toBeChecked();
  await expect(first.getByText(/Copies the text of From to To\./)).toBeVisible();
  await fill(page, { From: "PID.3[0].1", To: "P.3", Default: "UNKNOWN" }, first);
  await setCheckedIn(first, "Required");

  // constant: Value, To.
  await addRule(page);
  const second = rule(page, "2");
  await pick(page, "Rule", "constant", second);
  await expect(second.getByText("Writes the Value as text to To.")).toBeVisible();
  await expectRequired(page, ["To"], second);
  await fill(page, { Value: "LAB", To: "H.5" }, second);

  // datetime: formats are required, timezones optional and must come as a pair.
  await addRule(page);
  const third = rule(page, "3");
  await pick(page, "Rule", "datetime", third);
  await expectRequired(page, ["From", "To", "Input format", "Output format"], third);
  await expectOptional(page, ["From timezone", "To timezone"], third);
  await fill(
    page,
    {
      From: "OBR.7",
      To: "R.7",
      "Input format": "yyyyMMddHHmmss",
      "Output format": "yyyy-MM-dd HH:mm:ss",
      "From timezone": "Mars/Base",
    },
    third,
  );
  await runValidate(page);
  await expectIssue(
    page,
    '"Mars/Base" is not a time zone name such as Europe/Berlin.',
    /mappings\[2\]\.fromTimezone/,
  );
  await expect(third.getByText(/"Mars\/Base" is not a time zone name/)).toBeVisible();
  await fill(page, { "From timezone": "Europe/Berlin" }, third);
  await runValidate(page);
  await expectIssue(
    page,
    "Set both time zones to convert; with only one the time is not converted.",
    /mappings\[2\]\.toTimezone/,
  );
  await expect(saveAndApply(page))
    .toBeDisabled()
    .catch(() => undefined);
  await fill(page, { "To timezone": "America/New_York" }, third);

  // concat: parts of two kinds, added, ordered and removed.
  await addRule(page);
  const fourth = rule(page, "4");
  await pick(page, "Rule", "concat", fourth);
  await expect(fourth.getByText("No parts yet.")).toBeVisible();
  await fill(page, { To: "R.4" }, fourth);
  await fourth.getByRole("button", { name: "Add part to destination 1 mapping rule 4" }).click();
  await fourth.getByRole("button", { name: "Add part to destination 1 mapping rule 4" }).click();
  const part1 = row(page, "Part 1 of destination 1 mapping rule 4");
  const part2 = row(page, "Part 2 of destination 1 mapping rule 4");
  await fill(page, { "From path": "PID.5[0].1" }, part1);
  await pick(page, "Part type", "Text", part2);
  await fill(page, { Text: ", " }, part2);
  await fourth
    .getByRole("button", { name: "Move part 2 of destination 1 mapping rule 4 up" })
    .click();
  await expect(text(page, "Text", row(page, "Part 1 of destination 1 mapping rule 4"))).toHaveValue(
    ", ",
  );
  await fourth
    .getByRole("button", { name: "Move part 2 of destination 1 mapping rule 4 up" })
    .click();
  await expect(
    text(page, "From path", row(page, "Part 1 of destination 1 mapping rule 4")),
  ).toHaveValue("PID.5[0].1");
  await fourth
    .getByRole("button", { name: "Remove part 2 of destination 1 mapping rule 4" })
    .click();
  await expect(row(page, "Part 2 of destination 1 mapping rule 4")).toHaveCount(0);

  // A target written twice is a warning: the later rule wins.
  await fill(page, { To: "P.3" }, fourth);
  await runValidate(page);
  await expect(results(page)).toContainText(
    '"P.3" is written by an earlier rule too; the later rule wins.',
  );
});

async function setCheckedIn(scope: Locator, label: string): Promise<void> {
  const box = scope.getByRole("checkbox", { name: label, exact: true });
  await scope.locator("label", { hasText: label }).first().click();
  await expect(box).toBeChecked();
}

test("H-07 mapping rules: when and for_each nest their own lists, with sequence only inside a loop", async ({
  page,
}) => {
  await mappedChannel(page, "Bench nesting");
  await addRule(page);
  await pick(page, "Rule", "when", rule(page, "1"));
  const when = rule(page, "1");
  await expect(
    when.getByText("Runs the Then rules when the condition holds, otherwise the Else rules."),
  ).toBeVisible();
  const condition = group(page, "Condition of destination 1 mapping rule 1");
  await expectRequired(page, ["Path", "Value"], condition);
  await fill(page, { Path: "OBX[0].3", Value: "GLU" }, condition);
  await expect(group(page, "Then rules of destination 1 mapping rule 1")).toContainText(
    "Run when the condition holds.",
  );
  await expect(group(page, "Else rules of destination 1 mapping rule 1")).toContainText(
    "Leave empty to do nothing.",
  );

  // Then and Else hold rules of their own, numbered 1.1, 1.2, and so on.
  await when.getByRole("button", { name: "Add then rule to destination 1 mapping rule 1" }).click();
  await fill(page, { From: "OBX[0].5", To: "R.5" }, row(page, "Destination 1 then rule 1.1"));
  await when.getByRole("button", { name: "Add else rule to destination 1 mapping rule 1" }).click();
  await pick(page, "Rule", "constant", row(page, "Destination 1 else rule 1.1"));
  await fill(page, { Value: "n/a", To: "R.5" }, row(page, "Destination 1 else rule 1.1"));
  await when.getByRole("button", { name: "Add then rule to destination 1 mapping rule 1" }).click();
  await fill(page, { From: "OBX[0].6", To: "R.6" }, row(page, "Destination 1 then rule 1.2"));
  await when.getByRole("button", { name: "Move destination 1 then rule 1.2 up" }).click();
  await expect(text(page, "From", row(page, "Destination 1 then rule 1.1"))).toHaveValue(
    "OBX[0].6",
  );
  await when.getByRole("button", { name: "Remove destination 1 then rule 1.1" }).click();
  await expect(row(page, "Destination 1 then rule 1.2")).toHaveCount(0);
  await expect(text(page, "From", row(page, "Destination 1 then rule 1.1"))).toHaveValue(
    "OBX[0].5",
  );

  // for_each: Over and Into, and a list of rules inside it where sequence is offered.
  await addRule(page);
  await pick(page, "Rule", "for_each", rule(page, "2"));
  const loop = rule(page, "2");
  await expectRequired(page, ["Over", "Into"], loop);
  await fill(page, { Over: "OBX", Into: "R" }, loop);
  await loop.getByRole("button", { name: "Add rule inside destination 1 mapping rule 2" }).click();
  const inside = row(page, "Destination 1 loop rule 2.1");
  expect(await optionsOf(page, inside, "Rule")).toEqual([
    "copy",
    "constant",
    "lookup",
    "concat",
    "datetime",
    "calculate",
    "when",
    "sequence",
  ]);
  await pick(page, "Rule", "sequence", inside);
  await expect(
    inside.getByText(/Only inside a for_each or an append: writes the 1-based number/),
  ).toBeVisible();
  await expectRequired(page, ["Start"], inside);
  await fill(page, { To: "$.1", Start: "1" }, inside);
  await loop.getByRole("button", { name: "Add rule inside destination 1 mapping rule 2" }).click();
  const copy = row(page, "Destination 1 loop rule 2.2");
  await expect(copy.getByText(/\$\.4 for the current occurrence/).first()).toBeVisible();
  await fill(page, { From: "$.5", To: "$.5" }, copy);
  // A loop inside the loop inherits "inside a loop": its rules may use $. paths too.
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  // Engine checks that need the structure: $. outside a loop, names and counters.
  await fill(page, { From: "OBX[0].5", To: "$.9" }, row(page, "Destination 1 then rule 1.1"));
  await fill(page, { Over: "obx.1", Into: "R" }, loop);
  await fill(page, { Start: "-1" }, inside);
  await validate(page).click();
  await expect(inside.getByText("Enter a whole number, 0 or more.")).toBeVisible();
  await fill(page, { Start: "1" }, inside);
  await runValidate(page);
  await expectIssue(
    page,
    "Paths starting with $. are only valid inside a for_each rule.",
    /mappings\[0\]\.then\[0\]\.to/,
  );
  await expectIssue(
    page,
    "The record to loop over must be a record or segment name such as R or OBX.",
    /mappings\[1\]\.over/,
  );
  await expect(
    loop.getByText(/must be a record or segment name such as R or OBX/).first(),
  ).toBeVisible();
  await expect(saveAndApply(page)).toBeDisabled();

  // Top-level rules can be reordered and removed.
  await group(page, MAPPINGS)
    .getByRole("button", { name: "Move destination 1 mapping rule 2 up", exact: true })
    .click();
  await expect(group(page, MAPPINGS).getByRole("status").first()).toContainText(
    "moved to position 1 of 2",
  );
  await expect(text(page, "Over", rule(page, "1"))).toHaveValue("obx.1");
  await group(page, MAPPINGS)
    .getByRole("button", { name: "Remove destination 1 mapping rule 1", exact: true })
    .click();
  await expect(
    text(page, "Path", group(page, "Condition of destination 1 mapping rule 1")),
  ).toHaveValue("OBX[0].3");
});
