import { screen, waitFor, within } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { expect, type Mock } from "vitest";

type User = ReturnType<typeof userEvent.setup>;

/** Picks "JSON lines" in the export dialog's Format menu. */
export async function chooseJsonLines(user: User, dialog: HTMLElement) {
  await user.click(within(dialog).getByRole("button", { name: /Format/ }));
  await user.click(await screen.findByRole("option", { name: /JSON lines/ }));
}

/** Cancel closes the export dialog without calling the engine's export (`exportCall`). */
export async function cancelExportDialog(user: User, dialog: HTMLElement, exportCall: Mock) {
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(exportCall).not.toHaveBeenCalled();
}

/** The dialog closed after the OS save dialog was cancelled, with no "exported" notice. */
export async function expectClosedWithoutNotice(exportedTitle: string) {
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const notices = screen.getByRole("region", { name: "Notifications" });
  expect(within(notices).queryByText(exportedTitle)).toBeNull();
  expect(within(notices).queryByRole("listitem")).toBeNull();
}
