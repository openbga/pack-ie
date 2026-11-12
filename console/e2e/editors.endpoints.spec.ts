import { expect, test } from "@playwright/test";
import {
  expectIssue,
  expectOptional,
  expectRequired,
  fill,
  form,
  openEditor,
  openVersions,
  pick,
  results,
  reviewDialog,
  runValidate,
  saveAndApply,
  saveAndConfirm,
  select,
  setChecked,
  text,
  validate,
  versionRow,
} from "./editors.support";

test("H-07 serial endpoint: fields, required marks, client messages, then applied as a new version", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await expect(page.getByText("Choose a kind to see its connection settings.")).toBeVisible();
  await pick(page, "Kind", "Serial");
  await expectRequired(page, ["Label", "Id", "Baud rate"]);
  // The port is a picker of the computer's ports that also takes a typed name (serialPorts.spec.ts).
  const port = form(page).getByRole("combobox", { name: /^Port/ });
  await expect(port).toHaveAttribute("aria-required", "true");
  for (const label of ["Data bits", "Parity", "Stop bits", "Flow control"]) {
    await expect(select(page, label)).toBeVisible();
  }
  await expect(form(page).getByRole("checkbox", { name: "Enabled" })).toBeChecked();

  // Client-side validation names what is missing, at the field.
  await validate(page).click();
  await expect(form(page).getByText("Enter label.")).toBeVisible();
  await expect(form(page).getByText("Enter id.")).toBeVisible();
  await expect(form(page).getByText("Enter port.")).toBeVisible();
  await fill(page, { Label: "Bench serial 77", "Baud rate": "fast" });
  await port.fill("COM77");
  await port.press("Tab");
  await expect(text(page, "Id")).toHaveValue("bench-serial-77");
  await validate(page).click();
  await expect(form(page).getByText("Enter a whole number, 1 or more.")).toBeVisible();

  // The engine's own check: an id that is taken.
  await fill(page, { "Baud rate": "19200", Id: "ep-lis-out" });
  await runValidate(page);
  await expectIssue(
    page,
    'Duplicate endpoint id "ep-lis-out".',
    /endpoints\[\d+\]\.id · duplicate_id/,
  );
  await expect(form(page).getByText('Duplicate endpoint id "ep-lis-out".').first()).toBeVisible();
  await expect(saveAndApply(page)).toBeDisabled();
  await expect(results(page)).toContainText("Save and apply stays off until you fix them");

  await fill(page, { Id: "bench-serial-77" });
  await pick(page, "Parity", "Even");
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await expect(saveAndApply(page)).toBeEnabled();
  await saveAndApply(page).click();

  // The plan: a new endpoint restarts nothing.
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Endpoints added: Bench serial 77")).toBeVisible();
  await expect(dialog.getByText("No channel is restarted.")).toBeVisible();
  await expect(dialog.getByText(/^Untouched: \d+ channels?, which keep running\.$/)).toBeVisible();
  // Nothing is applied without a reason.
  await dialog.getByRole("button", { name: /^Apply version \d+$/ }).click();
  await expect(dialog.getByText("Enter why you are making this change.")).toBeVisible();
  await expect(dialog.getByRole("textbox", { name: /^Change note/ })).toBeFocused();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Bench serial port");
  const next = Number(
    /version (\d+)\?/.exec((await dialog.getByRole("heading").first().textContent()) ?? "")?.[1],
  );
  await dialog.getByRole("button", { name: `Apply version ${next}` }).click();
  await expect(page.getByText(`Configuration version ${next} applied`)).toBeVisible();
  await expect(page).toHaveURL(/#\/endpoints\/bench-serial-77$/);

  await openVersions(page);
  await expect(versionRow(page, next)).toContainText("Current");
  await expect(versionRow(page, next)).toContainText("LAB\\jdoe");
  await expect(versionRow(page, next)).toContainText("Bench serial port");
});

test("H-07 TCP client endpoint: fields, required marks and port and timeout validation", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "TCP client");
  await expectRequired(page, [
    "Label",
    "Id",
    "Host",
    "Port",
    "Connect timeout (ms)",
    "Idle timeout (seconds)",
  ]);
  await expect(
    form(page).getByRole("checkbox", { name: "Send TCP keepalive probes" }),
  ).toBeChecked();
  await fill(page, { Label: "Bench client", Host: "bench.lab.local", Port: "70000" });
  await text(page, "Connect timeout (ms)").fill("0");
  await text(page, "Idle timeout (seconds)").fill("-1");
  await validate(page).click();
  await expect(form(page).getByText("Enter a whole number from 1 to 65535.")).toBeVisible();
  await expect(form(page).getByText("Enter a whole number, 1 or more.")).toBeVisible();
  await expect(form(page).getByText("Enter a whole number from 0 to 604800.")).toBeVisible();

  await fill(page, { Port: "2575", "Connect timeout (ms)": "3000", "Idle timeout (seconds)": "0" });
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await saveAndConfirm(page);
  await expect(page).toHaveURL(/#\/endpoints\/bench-client$/);
});

test("H-07 TCP listener endpoint: the allowlist rule, address validation and peer conflicts", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "TCP listener");
  await expectRequired(page, ["Label", "Id", "Bind address", "Port", "Idle timeout (seconds)"]);
  await expect(text(page, "Bind address")).toHaveValue("0.0.0.0");
  await expect(form(page).getByText("No peer allowlist")).toBeVisible();
  await fill(page, { Label: "Bench listener", Port: "9911" });

  // A listener reachable from the network needs an allowlist or the explicit opt-out.
  await runValidate(page);
  await expectIssue(
    page,
    /^List the peers allowed to connect/,
    /endpoints\[\d+\]\.allowlist · allowlist_required/,
  );
  await expect(form(page).getByRole("group", { name: "Peer allowlist" })).toContainText(
    "List the peers allowed to connect, or allow any peer",
  );
  await expect(saveAndApply(page)).toBeDisabled();

  // Address rows: add, validate in the browser, then in the engine, remove.
  await page.getByRole("button", { name: "Add address" }).click();
  const address = text(page, "IP address or range");
  await expect(address).toHaveAttribute("aria-required", "true");
  await address.fill("not-an-ip!");
  await validate(page).click();
  await expect(
    form(page).getByText("Enter an IP address or a range such as 10.0.4.0/24."),
  ).toBeVisible();
  await address.fill("999.1.1.1");
  await runValidate(page);
  await expectIssue(
    page,
    /"999\.1\.1\.1" is not an IP address or CIDR range/,
    /allowlist\[0\] · invalid_cidr/,
  );
  await expect(saveAndApply(page)).toBeDisabled();
  await address.fill("10.0.4.0/24");
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await expect(form(page).getByText("No peer allowlist")).toHaveCount(0);

  // The opt-out: no entries and "Allow any peer" is allowed, with a warning that does not block.
  await page.getByRole("button", { name: /^Remove address 1$/i }).click();
  await expect(form(page).getByText("No entries.")).toBeVisible();
  await setChecked(page, "Allow any peer to connect", true);
  await expect(
    form(page).getByText(/Any host that can reach this address and port may connect/),
  ).toBeVisible();
  await runValidate(page);
  await expect(results(page)).toContainText("Any peer on the network can connect.");
  await expect(results(page)).toContainText("Warnings do not block applying.");
  await expect(saveAndApply(page)).toBeEnabled();

  // Two enabled listeners cannot claim one address and port (an existing one: 10.0.4.5:2576).
  await fill(page, { "Bind address": "10.0.4.5", Port: "2576" });
  await runValidate(page);
  await expectIssue(
    page,
    "Another listener already uses 10.0.4.5:2576.",
    /endpoints\[\d+\]\.port · port_conflict/,
  );
  await expect(saveAndApply(page)).toBeDisabled();
  await fill(page, { Port: "2599" });
  await runValidate(page);
  await expect(saveAndApply(page)).toBeEnabled();
  await saveAndConfirm(page);
  await expect(page).toHaveURL(/#\/endpoints\/bench-listener$/);
});

test("H-07 folder endpoint: fields, poll interval only when polling, file pattern format", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "Folder");
  await expectRequired(page, ["Label", "Id", "Folder path", "File name pattern"]);
  await expect(text(page, "Poll interval (seconds)")).toHaveCount(0);
  await pick(page, "Mode", "Poll on a schedule");
  await expectRequired(page, ["Poll interval (seconds)"]);
  await pick(page, "Mode", "Watch for new files");
  await expect(text(page, "Poll interval (seconds)")).toHaveCount(0);

  await fill(page, {
    Label: "Bench folder",
    "Folder path": "/srv/lab/in",
    "File name pattern": "in/*.csv",
  });
  await validate(page).click();
  await expect(
    form(page)
      .getByText(/^Use only \* and \? as wildcards/)
      .first(),
  ).toBeVisible();
  await fill(page, { "File name pattern": "*.csv" });
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await saveAndConfirm(page);
  await expect(page).toHaveURL(/#\/endpoints\/bench-folder$/);
});

test("H-07 SFTP endpoint: secret names, host key fingerprint and what the engine says about them", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "SFTP");
  await expectRequired(page, [
    "Label",
    "Id",
    "Host",
    "Port",
    "User name",
    "Password secret name",
    "Host key fingerprint",
    "Remote path",
    "Poll interval (seconds)",
    "File name pattern",
  ]);
  await expect(text(page, "Port")).toHaveValue("22");
  await expect(
    form(page).getByText(
      /stored under this name in Settings › Secrets, never in the configuration/,
    ),
  ).toBeVisible();
  await expect(
    form(page).getByText(/the connection is refused if the server shows a different one/),
  ).toBeVisible();

  await fill(page, {
    Label: "Bench SFTP",
    Host: "sftp.lab.local",
    "User name": "labxfer",
    "Remote path": "/outbound",
    "Host key fingerprint": "SHA256:short",
  });
  await text(page, "Password secret name").fill("bench-sftp-password");
  await validate(page).click();
  await expect(
    form(page).getByText("Enter SHA256: followed by 43 base64 characters."),
  ).toBeVisible();
  const fingerprint = "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s";
  await text(page, "Host key fingerprint").fill(fingerprint);

  // A name no secret holds yet is a warning, not an error: it does not block applying.
  await runValidate(page);
  await expectIssue(
    page,
    'No secret named "bench-sftp-password" is stored yet.',
    /secretName · secret_unresolved/,
  );
  await expect(results(page)).toContainText("Warnings do not block applying.");
  await expect(saveAndApply(page)).toBeEnabled();

  // An existing secret can be chosen instead of typed.
  await pick(page, "Existing secrets for password secret name", "reflab-sftp-password");
  await expect(text(page, "Password secret name")).toHaveValue("reflab-sftp-password");
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  // A private key adds its own secret and an optional passphrase secret.
  await pick(page, "Authentication", "Private key");
  await expectRequired(page, ["Private key secret name"]);
  await expectOptional(page, ["Passphrase secret name"]);
  await text(page, "Passphrase secret name").fill("bench-key-passphrase");
  await runValidate(page);
  await expectIssue(
    page,
    'No secret named "bench-key-passphrase" is stored yet.',
    /passphraseSecretName · secret_unresolved/,
  );
  await text(page, "Passphrase secret name").fill("");
  await runValidate(page);

  await saveAndConfirm(page);
  await expect(page).toHaveURL(/#\/endpoints\/bench-sftp$/);
  void fingerprint;
});

test("H-07 editing an endpoint: kind and id are fixed, and the plan restarts the channels that use it", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  await expect(
    form(page)
      .getByRole("term")
      .filter({ hasText: /^Kind$/ }),
  ).toBeVisible();
  await expect(
    form(page)
      .getByRole("definition")
      .filter({ hasText: /^TCP client$/ }),
  ).toBeVisible();
  await expect(
    form(page)
      .getByRole("definition")
      .filter({ hasText: /^ep-lis-out$/ }),
  ).toBeVisible();
  await expect(select(page, "Kind")).toHaveCount(0);
  await expect(text(page, "Id")).toHaveCount(0);
  await expect(text(page, "Host")).toHaveValue("lis.lab.local");

  await fill(page, { Host: "lis2.lab.local" });
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Endpoints changed: LIS HL7 out")).toBeVisible();
  const restarted = dialog.getByRole("listitem").filter({ hasText: /^Restarted \(changed\): / });
  await expect(restarted).toContainText("Vitros 350 results");
  await expect(restarted).toContainText("Reference lab results");
  await expect(restarted).not.toContainText("Billing export");
  await expect(dialog.getByText(/^Untouched: \d+ channels?, which keep running\.$/)).toBeVisible();
  const next = Number(
    /version (\d+)\?/.exec((await dialog.getByRole("heading").first().textContent()) ?? "")?.[1],
  );
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Move the reference lab port");
  await dialog.getByRole("button", { name: `Apply version ${next}` }).click();
  await expect(page.getByText(`Configuration version ${next} applied`)).toBeVisible();
  await expect(
    page.getByText(/^Restarted: .*Vitros 350 results.*\. \d+ unchanged\.$/),
  ).toBeVisible();
  await expect(page).toHaveURL(/#\/endpoints\/ep-lis-out$/);
});
