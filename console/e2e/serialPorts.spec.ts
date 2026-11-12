import { expect, type Locator, type Page, test } from "@playwright/test";
import { expectNoAxeViolations } from "./a11y.support";
import { form, openEditor, runValidate, saveAndConfirm } from "./editors.support";
import { heading, mockHandle, open } from "./support";

// The serial endpoint's port is picked from the serial ports of the engine's computer
// (docs/spec/diagnostics.md §10). The mock computer has COM1 (PCI), COM3 (Bluetooth) and the USB
// adapters COM4, COM5 and COM9; the seeded configuration names COM4, COM5 and COM9 through three
// bridge analyzers. Journeys run in their own page, so what one applies never reaches another.

const AU5800 = "/endpoints/ep-a-au5800/edit";

const portField = (page: Page): Locator => form(page).getByRole("combobox", { name: /^Port/ });
const optionOf = (page: Page, port: string): Locator =>
  page.getByRole("option", { name: new RegExp(`^${port}`) });

/** Opens the list of ports once they are found. */
async function openList(page: Page): Promise<void> {
  await expect(portField(page)).toBeEnabled();
  await portField(page).click();
  await expect(page.getByRole("listbox")).toBeVisible();
}

/** Types a port name and leaves the field, as a person does before the next field. */
async function typePort(page: Page, name: string): Promise<void> {
  await portField(page).fill(name);
  await portField(page).press("Tab");
  await expect(page.getByRole("listbox")).toHaveCount(0);
}

test("SP-01 the editor lists the ports with kind, device and ids, and marks the ones other endpoints use", async ({
  page,
}) => {
  await openEditor(page, AU5800, /./);
  // Its own port is selected and is not another endpoint's.
  await expect(portField(page)).toHaveValue("COM4");
  await openList(page);
  await expect(page.getByRole("option")).toHaveText([
    /^COM1PCI · Communications Port$/,
    /^COM3Bluetooth · Standard Serial over Bluetooth link$/,
    /^COM4USB · FT232R USB UART \(FTDI\) · 0403:6001 · serial A50285BI$/,
    /^COM5USB · USB-Serial Controller \(Prolific Technology Inc\.\) · 067b:2303used by cobas c 503 \(COM5\) · open by ch-br-c503$/,
    /^COM9USB · USB-SERIAL CH340 \(wch\.cn\) · 1a86:7523used by ACL TOP 550 coag \(COM9\)$/,
  ]);
  await expect(optionOf(page, "COM4")).not.toContainText("used by");
  await expect(optionOf(page, "COM4")).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(
    form(page).getByText("On this computer: USB · FT232R USB UART (FTDI) · 0403:6001"),
  ).toBeVisible();
  await expect(form(page).getByText("Two endpoints naming one port")).toHaveCount(0);
  await expect(form(page).getByText("5 serial ports found on this computer.")).toBeVisible();

  // A new endpoint is nobody's owner: every use counts, the running channel too.
  await open(page, "/endpoints/new");
  await heading(page, "New endpoint").waitFor();
  await page.getByRole("button", { name: /Kind/ }).click();
  await page.getByRole("option", { name: "Serial", exact: true }).click();
  await openList(page);
  await expect(optionOf(page, "COM4")).toContainText(
    "used by AU5800 chem analyzer (COM4) · open by ch-br-au5800",
  );
});

test("SP-02 a listed port is picked, applied, and shown by the endpoint and by the next editor", async ({
  page,
}) => {
  await openEditor(page, AU5800, /./);
  await openList(page);
  await optionOf(page, "COM1").click();
  await expect(portField(page)).toHaveValue("COM1");
  await expect(form(page).getByText("On this computer: PCI · Communications Port.")).toBeVisible();
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await saveAndConfirm(page, "Move the AU5800 to the on-board port");

  await expect(page).toHaveURL(/#\/endpoints\/ep-a-au5800$/);
  await expect(page.getByText("COM1 · 19200 8N1").first()).toBeVisible();

  // The markers follow the active configuration: COM4 is free, COM1 is this endpoint's own.
  await page.evaluate(() => {
    window.location.hash = "/endpoints/ep-a-au5800/edit";
  });
  await expect(portField(page)).toHaveValue("COM1");
  await openList(page);
  await expect(optionOf(page, "COM4")).not.toContainText("used by");
  await expect(optionOf(page, "COM1")).not.toContainText("used by");
  await expect(optionOf(page, "COM5")).toContainText("used by cobas c 503 (COM5)");
});

test("SP-03 a port another endpoint names can be picked, with a warning that both cannot open it", async ({
  page,
}) => {
  await openEditor(page, AU5800, /./);
  await openList(page);
  await optionOf(page, "COM5").click();
  await expect(portField(page)).toHaveValue("COM5");
  await expect(
    form(page).getByText(
      "COM5 is already named by cobas c 503 (COM5). Two endpoints naming one port cannot both open it.",
    ),
  ).toBeVisible();
  // The engine says the same at validation, as a warning that does not stop the change.
  await runValidate(page);
  await expect(page.getByText("COM5 is also used by another endpoint.").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Save and apply" })).toBeEnabled();

  // Choosing a free port takes the warning away.
  await openList(page);
  await optionOf(page, "COM3").click();
  await expect(form(page).getByText("Two endpoints naming one port")).toHaveCount(0);
});

test("SP-04 a name that is not listed can be typed and applied, and is labelled as not found afterwards", async ({
  page,
}) => {
  await openEditor(page, AU5800, /./);
  await expect(portField(page)).toBeEnabled();
  await portField(page).fill("COM40");
  await expect(page.getByRole("option")).toHaveText([
    /^Use “COM40” anywayNot found on this computer$/,
  ]);
  await page.getByRole("option").click();
  await expect(portField(page)).toHaveValue("COM40");
  await expect(
    form(page).getByText(/COM40 was not found on this computer\. It is kept as typed/),
  ).toBeVisible();
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();
  await saveAndConfirm(page, "Port moved to an adapter that is unplugged");
  await expect(page.getByText("COM40 · 19200 8N1").first()).toBeVisible();

  await page.evaluate(() => {
    window.location.hash = "/endpoints/ep-a-au5800/edit";
  });
  await expect(portField(page)).toHaveValue("COM40");
  await expect(form(page).getByText(/COM40 was not found on this computer/)).toBeVisible();
  await openList(page);
  await expect(page.getByRole("option", { selected: true })).toContainText(
    "Use “COM40” anywayNot found on this computer",
  );
});

test("SP-05 a listing that fails offers Try again and leaves the form usable; Refresh scans again", async ({
  page,
}) => {
  await open(page, "/endpoints");
  await heading(page, "Endpoints").waitFor();
  await mockHandle(page, (handle) => handle.failSerialPorts(true));
  await page.evaluate(() => {
    window.location.hash = "/endpoints/ep-a-au5800/edit";
  });
  await expect(form(page)).toBeVisible();
  const failure = form(page)
    .getByRole("alert")
    .filter({ hasText: "could not list the serial ports" });
  await expect(failure).toContainText("You can still type the port name.");
  await failure.getByRole("button", { name: "Try again" }).click();
  await expect(failure).toBeVisible();

  // Typing a name works and the change can be applied.
  await expect(portField(page)).toBeEnabled();
  await typePort(page, "COM40");
  await expect(form(page).getByText(/COM40 was not found/)).toHaveCount(0);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  // The engine can list again: Try again finds the ports, and the typed name stays.
  await mockHandle(page, (handle) => handle.failSerialPorts(false));
  await failure.getByRole("button", { name: "Try again" }).click();
  await expect(form(page).getByText("5 serial ports found on this computer.")).toBeVisible();
  await expect(failure).toHaveCount(0);
  await expect(portField(page)).toHaveValue("COM40");
  await expect(form(page).getByText(/COM40 was not found on this computer/)).toBeVisible();

  // Refresh asks again; a listing that fails now is reported at once.
  await mockHandle(page, (handle) => handle.failSerialPorts(true));
  await form(page).getByRole("button", { name: "Refresh" }).click();
  await expect(failure).toBeVisible();
  await saveAndConfirm(page, "Typed port with the listing down");
  await expect(page.getByText("COM40 · 19200 8N1").first()).toBeVisible();
});

test("SP-06 the picker has no accessibility violations, closed or open", async ({ page }) => {
  await openEditor(page, AU5800, /./);
  await expect(portField(page)).toBeEnabled();
  // The page behind an open list is hidden from assistive technology by design: check the list itself.
  await expectNoAxeViolations(page, { selector: "form" });
  await openList(page);
  await expect(page.getByRole("option")).toHaveCount(5);
  await expectNoAxeViolations(page, { selector: "[role=listbox]" });
  await page.keyboard.press("Escape");
  await expectNoAxeViolations(page, { selector: "form" });
});
