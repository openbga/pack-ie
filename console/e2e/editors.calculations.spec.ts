import { expect, test } from "@playwright/test";
import {
  addRule,
  expectIssue,
  fill,
  general,
  group,
  mappedChannel,
  openEditor,
  pick,
  pickRecord,
  results,
  row,
  rule,
  runValidate,
  saveAndConfirm,
  select,
  text,
  validate,
} from "./editors.support";

test("I-01 a unit conversion in a loop: formula, rounding, comparator and the engine's own check", async ({
  page,
}) => {
  await mappedChannel(page, "Bench calculation");
  await addRule(page);
  const loop = rule(page, "1");
  await pick(page, "Rule", "for_each", loop);
  await fill(page, { Over: "R", Into: "OBX" }, loop);
  await loop.getByRole("button", { name: "Add rule inside destination 1 mapping rule 1" }).click();
  const calc = row(page, "Destination 1 loop rule 1.1");
  await pick(page, "Rule", "calculate", calc);
  await expect(calc.getByText(/Calculates a number with exact decimal arithmetic/)).toBeVisible();

  // A formula that does not read says where, and Validate stops before the engine is asked.
  await fill(page, { Formula: "$.4 *", To: "$.5" }, calc);
  await page.getByRole("button", { name: "Validate" }).click();
  await expect(calc.getByText(/^Expected a number, a path or a bracket at the end/)).toBeVisible();

  // The formula is shown the way the engine reads it.
  await fill(page, { Formula: "$.4 * 0.0555 / 2" }, calc);
  await expect(calc.getByRole("status").filter({ hasText: /^Reads as/ })).toHaveText(
    "Reads as $.4 × 0.0555 ÷ 2. Takes $.4 from the message.",
  );

  // A division needs decimal places; the engine's answer lands on the field.
  await runValidate(page);
  await expectIssue(page, "A formula with a division needs decimal places.", /scale/);
  await fill(page, { "Decimal places": "2" }, calc);
  await pick(page, "Rounding", /^Half even/, calc);
  await pick(page, "When it cannot be calculated", "Write nothing", calc);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
});

test("I-02 an anion gap: named operands that search for a test code, appended as a new result", async ({
  page,
}) => {
  await mappedChannel(page, "Bench anion gap");
  await addRule(page);
  const append = rule(page, "1");
  await pick(page, "Rule", "append", append);
  await fill(page, { Into: "OBX" }, append);
  await append
    .getByRole("button", { name: "Add rule to the record added by destination 1 mapping rule 1" })
    .click();
  const calc = row(page, "Destination 1 appended rule 1.1");
  await pick(page, "Rule", "calculate", calc);
  await fill(page, { Formula: "@na - (@cl + @hco3)", To: "$.5" }, calc);
  await page.getByRole("button", { name: "Validate" }).click();
  await expect(calc.getByText(/^No operand is named @na/)).toBeVisible();

  for (const [index, code] of ["NA", "CL", "HCO3"].entries()) {
    await calc
      .getByRole("button", { name: "Add operand to destination 1 appended rule 1.1" })
      .click();
    const operand = row(page, `Operand ${index + 1} of destination 1 appended rule 1.1`);
    await pick(page, "Reads", /^The value in the first matching record/, operand);
    await fill(
      page,
      {
        Name: code.toLowerCase(),
        "Record or segment": "OBX",
        "Value path": "$.5",
        Path: "$.3.1",
        Value: code,
      },
      operand,
    );
  }
  await expect(calc.getByRole("status").filter({ hasText: /^Reads as/ })).toContainText(
    "Reads as @na − (@cl + @hco3).",
  );
  await pick(page, "When it cannot be calculated", "Write nothing", calc);

  // The rest of the record: number, type, test code.
  const addInside = append.getByRole("button", {
    name: "Add rule to the record added by destination 1 mapping rule 1",
  });
  await addInside.click();
  const sequence = row(page, "Destination 1 appended rule 1.2");
  await pick(page, "Rule", "sequence", sequence);
  await fill(page, { To: "$.1" }, sequence);
  await addInside.click();
  const constant = row(page, "Destination 1 appended rule 1.3");
  await pick(page, "Rule", "constant", constant);
  await fill(page, { Value: "AG^Anion gap", To: "$.3" }, constant);

  // A To that is not a path of the new record is stopped.
  await addInside.click();
  const wrong = row(page, "Destination 1 appended rule 1.4");
  await pick(page, "Rule", "constant", wrong);
  await fill(page, { Value: "mmol/L", To: "OBX[0].6" }, wrong);
  await runValidate(page);
  await expectIssue(page, "In an appended record, To is a path starting $.", /rules\[3\]\.to/);
  await fill(page, { To: "$.6" }, wrong);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await saveAndConfirm(page, "Anion gap");
});

test("I-03 text steps, ignore rules as combined conditions, and a table pasted as text", async ({
  page,
}) => {
  await mappedChannel(page, "Bench steps");
  // Steps on a copy rule.
  await addRule(page);
  const copy = rule(page, "1");
  await fill(page, { From: "PID.3", To: "P.3" }, copy);
  await copy.getByRole("button", { name: "Add text step to destination 1 mapping rule 1" }).click();
  const step = row(page, "Step 1 of destination 1 mapping rule 1");
  await pick(page, "Step", "Take one part of a split", step);
  await fill(page, { Separator: "/", "Part number": "-1" }, step);
  await copy.getByRole("button", { name: "Add text step to destination 1 mapping rule 1" }).click();
  await pick(
    page,
    "Step",
    "Remove leading characters",
    row(page, "Step 2 of destination 1 mapping rule 1"),
  );
  await fill(page, { Characters: "0" }, row(page, "Step 2 of destination 1 mapping rule 1"));

  // Skip control samples: not any of two prefixes, on the destination.
  await page.getByRole("button", { name: "Add destination 1 filter" }).click();
  const filter = row(page, "Destination 1 filter 1");
  await pick(page, "Condition", "This does not hold", filter);
  const inner = row(page, "Not destination 1 filter 1");
  await pick(page, "Condition", "Any of these holds", inner);
  // The comparison there was becomes the first condition; one more makes two.
  await page
    .getByRole("button", { name: "Add condition to negated condition of destination 1 filter 1" })
    .click();
  for (const [index, prefix] of ["CTL", "QC"].entries()) {
    const comparison = row(
      page,
      `Condition ${index + 1} of negated condition of destination 1 filter 1`,
    );
    await pick(page, "Operator", "starts with", comparison);
    await fill(page, { Path: "O.3", Value: prefix }, comparison);
  }

  // A table pasted from a spreadsheet, checked before it replaces anything.
  await page.getByRole("button", { name: "Add destination 1 code table" }).click();
  await fill(page, { "Table name": "sex" }, row(page, "Destination 1 table 1"));
  await page.getByRole("button", { name: "Edit destination 1 table 1 as text" }).click();
  const dialog = page.getByRole("dialog", { name: "Edit destination 1 table 1 as text" });
  await dialog.getByRole("textbox", { name: "Rows" }).fill("M\tMale\nF\tFemale\nnonsense");
  await expect(dialog.getByRole("alert")).toContainText("Line 3: Separate the code from its text");
  await expect(dialog.getByRole("button", { name: "Replace rows" })).toBeDisabled();
  await dialog
    .getByRole("textbox", { name: "Rows" })
    .fill("M\tMale\nF\tFemale\nU,Unknown, not stated");
  await expect(dialog.getByRole("status")).toHaveText(
    "3 rows: 3 new, 0 changed, 0 removed, 0 unchanged.",
  );
  await dialog.getByRole("button", { name: "Replace rows" }).click();
  await expect(
    row(page, "Row 3 of destination 1 table 1").getByRole("textbox", { name: "Text" }),
  ).toHaveValue("Unknown, not stated");
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
});

test("I-04 instrument presets: settings listed before they change, and a mapping that never overwrites", async ({
  page,
}) => {
  await openEditor(page, "/channels/new", "New channel");
  await pick(page, "Kind", "Message");
  await fill(page, { Label: "Bench presets" }, general(page));
  const source = group(page, "Source");
  await pickRecord(page, "Endpoint", "ep-xn1000", source);
  await pick(page, "Protocol", "ASTM", source);
  const destination = group(page, "Destination 1");
  await fill(page, { Label: "To LIS" }, destination);
  await pickRecord(page, "Endpoint", "ep-lis-out", destination);
  await pick(page, "Protocol", "HL7 v2", destination);

  // The settings the instrument contract added are ordinary fields with their defaults.
  await expect(text(page, "Characters per frame", source)).toHaveValue("240");
  await expect(text(page, "HL7 version", destination)).toHaveValue("2.5.1");
  await expect(text(page, "Accepted answer codes", destination)).toHaveValue("ACK");

  // A connector preset: what changes is listed, then written into the visible field.
  await source.getByRole("button", { name: "Instrument presets for the source" }).click();
  const presets = page.getByRole("dialog", { name: "Instrument presets" });
  await select(page, "Preset", presets).click();
  await page.getByRole("option", { name: /long frames/i }).click();
  await expect(presets.getByRole("row", { name: /Characters per frame/ })).toContainText("64000");
  await presets.getByRole("button", { name: "Apply 1 setting" }).click();
  await expect(text(page, "Characters per frame", source)).toHaveValue("64000");
  await expect(page.getByText(/applied$/)).toBeVisible();

  // The destination's mapping: the operator's own rule first, then the preset added after it.
  await pick(page, "Link role", "Computer", source);
  await addRule(page);
  await fill(page, { From: "P.3", To: "PID.3" }, rule(page, "1"));
  await page.getByRole("button", { name: "Start destination 1 from a preset" }).click();
  const mapping = page.getByRole("dialog", { name: "Mapping presets for destination 1" });
  // The preset's limits and assumptions are shown beside it.
  await expect(mapping.getByRole("listitem").first()).toBeVisible();
  await expect(mapping.getByRole("status")).toContainText("This adds");
  await mapping.getByText("Example input and output").click();
  await expect(mapping.getByText(/ORU\^R01/).first()).toBeVisible();
  await mapping.getByRole("button", { name: "Add to the mapping" }).click();
  await expect(rule(page, "1").getByRole("textbox", { name: "From" })).toHaveValue("P.3");
  await expect(row(page, "Destination 1 mapping rule 2")).toBeVisible();

  // Replacing says what goes and is a deliberate choice.
  await page.getByRole("button", { name: "Start destination 1 from a preset" }).click();
  const again = page.getByRole("dialog", { name: "Mapping presets for destination 1" });
  await select(page, "You already have rules or tables", again).click();
  await page.getByRole("option", { name: /Replace my rules/ }).click();
  await expect(again.getByRole("alert")).toContainText("This removes what you have here");
  await again.getByRole("button", { name: "Cancel" }).click();
  await expect(
    row(page, "Destination 1 mapping rule 1").getByRole("textbox", { name: "From" }),
  ).toHaveValue("P.3");

  // The operator's PID.3 and the preset's PID.3 collide: a warning names it, nothing is blocked.
  await runValidate(page);
  await expect(results(page)).toContainText('"PID.3" is written by an earlier rule too');
});

test("I-05 an order download: source requirements and one field written as repetitions", async ({
  page,
}) => {
  await mappedChannel(page, "Bench order download");
  const destination = group(page, "Destination 1");
  await destination.getByRole("button", { name: "Add requirement to destination 1" }).click();
  const count = row(page, "Requirement 1 of destination 1");
  await fill(page, { "Record or segment": "OBR", Minimum: "1", Maximum: "18" }, count);
  await destination.getByRole("button", { name: "Add requirement to destination 1" }).click();
  const same = row(page, "Requirement 2 of destination 1");
  await pick(page, "Check", "Same value in every record", same);
  await fill(page, { Path: "OBR.3.1" }, same);

  await addRule(page);
  const loop = rule(page, "1");
  await pick(page, "Rule", "for_each", loop);
  await pick(page, "Records", /^Write each one as a repetition/, loop);
  await fill(page, { Over: "OBR", "Into field": "O.5" }, loop);
  await loop.getByRole("button", { name: "Add rule inside destination 1 mapping rule 1" }).click();
  const copy = row(page, "Destination 1 loop rule 1.1");
  await fill(page, { From: "$.4.1", To: "$.5.4" }, copy);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  // The engine's own check of the field path lands on Into field.
  await fill(page, { "Into field": "OBX" }, loop);
  await runValidate(page);
  await expectIssue(page, "With repeat, Into is a field path such as O.5.", /mappings\[0\]\.into/);
});

test("I-06 an order download preset brings its source requirements and repeat loop, shown before applying", async ({
  page,
}) => {
  await mappedChannel(page, "Bench order preset");
  await page.getByRole("button", { name: "Start destination 1 from a preset" }).click();
  const dialog = page.getByRole("dialog", { name: "Mapping presets for destination 1" });
  await expect(dialog.getByRole("status")).toContainText("This adds");
  await expect(dialog.getByText(/source requirement/)).toBeVisible();
  await dialog.getByRole("button", { name: "Add to the mapping" }).click();
  const first = row(page, "Requirement 1 of destination 1");
  await expect(first).toBeVisible();
  await expect(text(page, "Record or segment", first)).toHaveValue("PID");
  await expect(
    page.getByRole("button", { name: /Write each one as a repetition/ }).first(),
  ).toBeVisible();
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
});

test("I-07 an instrument that carries the specimen id in O-4: the source path, its preset, and the mapping preset's pointer", async ({
  page,
}) => {
  await openEditor(page, "/channels/new", "New channel");
  await pick(page, "Kind", "Message");
  await fill(page, { Label: "Bench specimen id" }, general(page));
  const source = group(page, "Source");
  await pickRecord(page, "Endpoint", "ep-xn1000", source);
  await pick(page, "Protocol", "ASTM", source);
  const destination = group(page, "Destination 1");
  await fill(page, { Label: "To LIS" }, destination);
  await pickRecord(page, "Endpoint", "ep-lis-out", destination);
  await pick(page, "Protocol", "HL7 v2", destination);

  // The default, and the help that says what the path is for.
  await expect(text(page, "Sample id path", source)).toHaveValue("O.3");
  await expect(source.getByText(/O\.4\.3 is component 3 of field 4/)).toBeVisible();
  await expect(text(page, "Sample id path", destination)).toHaveCount(0);

  // The mapping preset for this layout says the source has to match.
  await page.getByRole("button", { name: "Start destination 1 from a preset" }).click();
  const mapping = page.getByRole("dialog", { name: "Mapping presets for destination 1" });
  await select(page, "Preset", mapping).click();
  await page.getByRole("option", { name: /specimen id in O-4/ }).click();
  await expect(
    mapping.getByText(/The example reads the source with Sample id path O\.4\.3/),
  ).toBeVisible();
  await expect(mapping.getByText(/Apply the source preset/)).toBeVisible();
  await mapping.getByRole("button", { name: "Add to the mapping" }).click();

  // The source preset writes the path into the visible field.
  await source.getByRole("button", { name: "Instrument presets for the source" }).click();
  const presets = page.getByRole("dialog", { name: "Instrument presets" });
  await select(page, "Preset", presets).click();
  await page.getByRole("option", { name: /specimen id/i }).click();
  await expect(presets.getByRole("row", { name: /Sample id path/ })).toContainText("O.4.3");
  await presets.getByRole("button", { name: "Apply 1 setting" }).click();
  await expect(text(page, "Sample id path", source)).toHaveValue("O.4.3");

  // The engine's syntax is checked at the field.
  await fill(page, { "Sample id path": "O.2" }, source);
  await validate(page).click();
  await expect(source.getByText(/^Use O\. and a field from 3 to 99/)).toBeVisible();
  await fill(page, { "Sample id path": "O.4.3" }, source);
  await runValidate(page);
  await expect(page.getByText(/No problems found|Warnings/).first()).toBeVisible();
});
